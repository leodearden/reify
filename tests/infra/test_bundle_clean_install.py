#!/usr/bin/env python3
"""
test_bundle_clean_install.py — the clean-machine proof of the Reify installer
(task 9732, docs/prds/alpha-spine.md D6 / §5.2 / boundary row B7).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered
member is the thin wrapper tests/infra/test_bundle_clean_install.sh, which
invokes this file.

ARMED ONLY: every test skips unless REIFY_RUN_BUNDLE_CLEAN_INSTALL=1, because
the proof is a cold bundle build plus two docker containers. Once armed, a
docker that cannot create a container within scripts/docker-create-probe.sh's
bound is a FAIL, not a skip.

What it proves, each in a fresh stock ubuntu:24.04 container whose premises are
asserted first (no /opt/reify-deps, only the stock apt sources):
  * the .deb installs with apt alone, no OCCT/gmsh/OpenVDB/oneTBB package gets
    pulled in (the bundle, not a Depends, must carry them), `reify check` on
    examples/m5_geometry.ri exits 0, and `reify-gui` under xvfb answers a
    debug-MCP store_state with at least one mesh; nodejs and python3 arrive as
    its Depends, the installed reify-sidecar completes the ready handshake, and
    an Assistant turn through it runs claude inside the landlock sandbox;
  * the AppImage does the same on a desktop-session stand-in (xvfb, xauth,
    curl, GTK 3 and the GL front-ends, but no WebKit, which the AppImage must
    carry itself); it has no node, and after `apt install nodejs` (the
    E_GUI_SIDECAR_NODE_MISSING remedy) its reify-sidecar completes the handshake;
    with python3 added, as a desktop ships it, its Assistant turn runs claude
    inside the landlock sandbox;
  * every shipped surface reports the version the assets were built at, the
    deb's control Version (task 10485): the installed `reify --version` and
    `reify-gui --version` (the AppImage's run through the AppImage itself, which
    is also named for that version) and the rendering GUI's debug-MCP
    initialize serverInfo.version;
  * reify-gui answers its own command line before any window exists (task
    10487), on the deb and through the AppImage itself: `--help` prints usage
    and exits 0 in a container with no display, and `cli --version` runs the
    bundled reify CLI. The AppImage's `cli check` and its GUI launch take a
    path relative to the caller's directory, though AppRun.wrapped chdirs into
    $APPDIR/usr before it execs reify-gui.

The sandboxed turn: the sidecar gets REIFY_LANDLOCK_EXEC at the helper's
installed path (tauri's Linux resource dir, where gui/src-tauri/src/main.rs
looks), and a stand-in `claude` reports whether it could write inside its
workspace and outside every writable root. A sidecar that could not sandbox
answers a sandbox_unavailable notice and outside=written instead.

Knobs: REIFY_BUNDLE_DEB + REIFY_BUNDLE_APPIMAGE reuse a prior build instead of
running scripts/build-bundle.sh; REIFY_BUNDLE_PROOF_READY_SECS (default 600) is
the hang guard on the store_state readiness poll — no elapsed time is asserted.
scripts/release-alpha.sh arms the proof with both reuse knobs over the assets it
is about to publish (B7 before any tag, task 10079).
"""

import json
import os
import posixpath
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
DOCKER_CREATE_PROBE = REPO_ROOT / "scripts" / "docker-create-probe.sh"

ARM_VAR = "REIFY_RUN_BUNDLE_CLEAN_INSTALL"
BASE_IMAGE = "ubuntu:24.04"
EXAMPLE = "/work/examples/m5_geometry.ri"
EXAMPLE_DIR, EXAMPLE_NAME = posixpath.split(EXAMPLE)
# Only this marker is asserted here; gui/src-tauri/tests/command_line.rs pins
# the whole --help text against command_line::USAGE.
USAGE_PREFIX = "Usage: "
DEBUG_PORT = 3939
GUI_ENV = {
    "REIFY_DEBUG": "1",
    "REIFY_DEBUG_PORT": str(DEBUG_PORT),
}
GUI_LOG = "/tmp/reify-gui.log"
SPLIT_STDERR = "/tmp/reify-split.err"
GUI_EXIT = "/tmp/reify-gui.exit"
SIDECAR_ERR = "/tmp/reify-sidecar.err"
# The readiness claude_bridge::ensure_sidecar_ready waits for.
SIDECAR_READY = {"type": "ready"}
# tauri's Linux resource_dir (<exe_dir>/../lib/<productName>) joined with the
# sandbox/landlock_exec.py gui/src-tauri/src/main.rs looks up, relative to the
# package root ("/" for the deb, squashfs-root/ for the AppImage).
LANDLOCK_EXEC = "usr/lib/Reify/sandbox/landlock_exec.py"
TURN = "t1"
TURN_ENDS = ("done", "error")
SANDBOX_MARKER = "reify-bundle-proof-sandbox-probe"
# landlock_exec grants /tmp wholesale and names /var/tmp as staying restricted.
OUTSIDE_PROBE = f"/var/tmp/{SANDBOX_MARKER}"
# The `claude` the sidecar spawns: drains the prompt, tries one write inside its
# workspace and one outside every writable root, and answers in stream-json
# (the reply shape gui/sidecar/src/__tests__/built-bundle.test.ts pins).
FAKE_CLAUDE = "\n".join([
    "#!/bin/sh",
    "cat >/dev/null",
    'probe() { if touch "$1" 2>/dev/null; then echo written; else echo denied; fi; }',
    f'w=$(probe "$REIFY_WORKSPACE/{SANDBOX_MARKER}")',
    f"o=$(probe {OUTSIDE_PROBE})",
    "cat <<EVENTS",
    json.dumps({"type": "assistant", "message": {
        "id": "m1", "content": [{"type": "text", "text": "workspace=$w outside=$o"}]}}),
    json.dumps({"type": "result", "session_id": "s1"}),
    "EVENTS",
    "",
])
SANDBOXED_TURN = [
    {"type": "text_delta", "id": TURN, "content": "workspace=written outside=denied"},
    {"type": "done", "id": TURN},
]
STORE_STATE_BODY = json.dumps({
    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
    "params": {"name": "store_state", "arguments": {}},
})
INITIALIZE_BODY = json.dumps({"jsonrpc": "2.0", "id": 0, "method": "initialize"})
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


def _first_json_line(text):
    """The first line of `text` parsed as JSON, or None when it is not JSON."""
    first_line = next(iter(text.splitlines()), "")
    try:
        return json.loads(first_line)
    except ValueError:
        return None


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

    def run_split(self, command):
        """Runs `command` with its stderr kept apart from its stdout. Returns
        the process, whose stdout is the command's alone, and that stderr."""
        proc = self.run(f"{command} 2>{SPLIT_STDERR}")
        return proc, self.run(f"cat {SPLIT_STDERR}").stdout

    def assert_reports(self, command, expected):
        """`command` exits 0 and its stdout, its stderr kept apart, is the line
        `expected`. Either failure shows that stderr, where a loader error lands."""
        proc, stderr = self.run_split(command)
        reported = proc.stdout.strip()
        self.test.assertEqual(
            (proc.returncode, reported), (0, expected),
            f"in {self.name}: {command}\nstderr:\n{stderr[-2000:]}")
        _log(f"{self.name}: {command} reports {reported!r}")

    def assert_usage(self, command):
        """`command` exits 0 and its first stdout line, its stderr kept apart,
        begins USAGE_PREFIX. Either failure shows that stderr."""
        proc, stderr = self.run_split(command)
        first_line = next(iter(proc.stdout.splitlines()), "")
        self.test.assertEqual(
            (proc.returncode, first_line.startswith(USAGE_PREFIX)), (0, True),
            f"in {self.name}: {command}\nstdout:\n{proc.stdout[-2000:]}\n"
            f"stderr:\n{stderr[-2000:]}")
        _log(f"{self.name}: {command} prints {first_line!r}")

    def assert_debug_server_reports(self, expected):
        """The running GUI's debug-MCP initialize answers serverInfo.version `expected`."""
        reply = self.check(
            f"curl -fsS --max-time 5 -X POST http://127.0.0.1:{DEBUG_PORT}/mcp "
            f"-H 'Content-Type: application/json' -d {shlex.quote(INITIALIZE_BODY)}")
        try:
            reported = json.loads(reply)["result"]["serverInfo"]["version"]
        except (ValueError, KeyError, TypeError):
            self.test.fail(f"in {self.name}: initialize reply has no serverInfo.version: "
                           f"{reply[-2000:]!r}")
        self.test.assertEqual(reported, expected,
                              f"in {self.name}: debug-MCP initialize serverInfo.version")
        _log(f"{self.name}: debug-MCP initialize reports serverInfo.version {reported!r}")

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

    def assert_sidecar_ready(self, sidecar):
        """Execs the installed sidecar directly, as the host does, from a dir
        with no node_modules and with stdin closed, and asserts it answers
        SIDECAR_READY and exits 0. `timeout` is a hang guard only."""
        proc = self.run(
            f'cd "$(mktemp -d)" && REIFY_WORKSPACE="$PWD" '
            f"timeout {int(_ready_secs())} {shlex.quote(sidecar)} </dev/null 2>{SIDECAR_ERR}")
        first = _first_json_line(proc.stdout)
        stderr = self.run(f"cat {SIDECAR_ERR}").stdout
        self.test.assertEqual(
            (proc.returncode, first), (0, SIDECAR_READY),
            f"in {self.name}: {sidecar}\nstdout:\n{proc.stdout[-4000:]}\nstderr:\n{stderr[-4000:]}")
        _log(f"{self.name}: {sidecar} answered {json.dumps(first)}, exit {proc.returncode}")

    def sidecar_turn(self, command):
        """Runs `command`, a sidecar launch, under `docker exec -i`; sends one
        send_message for TURN once the sidecar answers SIDECAR_READY, and
        returns (every outbound message up to the turn's end, how the run
        ended). Stdin closes only after the turn ends, because the sidecar
        aborts an in-flight turn at stdin EOF. The caller's in-container
        `timeout` is the hang guard; the wait here is only its backstop."""
        argv = ["docker", "exec", "-i", self.name, "bash", "-c", command]
        with (tempfile.TemporaryFile(mode="w+", encoding="utf-8") as docker_err,
              subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=docker_err, text=True) as proc):
            try:
                messages = self._read_turn(proc)
            finally:
                proc.stdin.close()
                try:
                    proc.wait(timeout=_ready_secs())
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
            docker_err.seek(0)
            return messages, f"docker exec exit {proc.returncode}; its stderr: {docker_err.read()[-2000:]!r}"

    def _read_turn(self, proc):
        messages = []
        for line in iter(proc.stdout.readline, ""):
            try:
                message = json.loads(line)
            except ValueError:
                message = None
            if not isinstance(message, dict):
                self.test.fail(f"in {self.name}: sidecar line {line!r} is no JSON object; "
                               f"read before it: {messages}")
            messages.append(message)
            if message == SIDECAR_READY:
                proc.stdin.write(json.dumps(
                    {"type": "send_message", "id": TURN, "text": "where can you write?"}) + "\n")
                proc.stdin.flush()
            elif message.get("id") == TURN and message.get("type") in TURN_ENDS:
                break
        return messages

    def assert_assistant_sandboxed(self, sidecar, landlock_exec):
        """Drives one Assistant turn through the installed sidecar, with
        REIFY_LANDLOCK_EXEC where the host finds the helper and the FAKE_CLAUDE
        stand-in on PATH, and asserts claude ran sandboxed: it wrote inside its
        workspace (outside /tmp, so that write is not the /tmp grant) and was
        denied outside every writable root."""
        self.check(f"test -f {shlex.quote(landlock_exec)}")
        self.check(f"printf %s {shlex.quote(FAKE_CLAUDE)} > /usr/local/bin/claude"
                   " && chmod 755 /usr/local/bin/claude")
        workspace = shlex.quote(self.check("mktemp -d -p /srv").strip())
        messages, ended = self.sidecar_turn(
            f"cd {workspace} && REIFY_WORKSPACE={workspace} "
            f"REIFY_LANDLOCK_EXEC={shlex.quote(landlock_exec)} "
            f"timeout {int(_ready_secs())} {shlex.quote(sidecar)} 2>{SIDECAR_ERR}")
        stderr = self.run(f"cat {SIDECAR_ERR}").stdout
        self.test.assertEqual(
            [m for m in messages if m.get("id") == TURN], SANDBOXED_TURN,
            f"in {self.name}: {sidecar} with {landlock_exec}\n{ended}\n"
            f"messages: {json.dumps(messages)}\nstderr:\n{stderr[-4000:]}")
        self.check(f"test ! -e {OUTSIDE_PROBE}")
        _log(f"{self.name}: {sidecar} ran claude inside the landlock sandbox")

    def gui_postmortem(self):
        proc = self.run(f"cat {GUI_EXIT} 2>/dev/null; tail -n 80 {GUI_LOG}")
        return f"GUI exit/log:\n{proc.stdout}"


class BundleCleanInstall(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        if os.environ.get(ARM_VAR) != "1":
            raise unittest.SkipTest(f"armed only: set {ARM_VAR}=1 (docker + a cold bundle build)")
        create_check = subprocess.run(
            ["bash", str(DOCKER_CREATE_PROBE), BASE_IMAGE],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, check=False,
        )
        if create_check.returncode != 0:
            raise AssertionError(
                "armed, but the docker preflight failed "
                f"(scripts/docker-create-probe.sh exit {create_check.returncode}):\n"
                f"{create_check.stdout[-2000:]}")
        reused = (os.environ.get("REIFY_BUNDLE_DEB"), os.environ.get("REIFY_BUNDLE_APPIMAGE"))
        if all(reused):
            cls.build = None
            cls.deb, cls.appimage = (Path(p) for p in reused)
        else:
            cls.build = cls._run_build_bundle()
            cls.deb, cls.appimage = cls._assets_from(cls.build.stdout)
        cls.version = cls._deb_control_version(cls.deb)
        _log(f"the assets' version, from {cls.deb.name}'s control Version: {cls.version}")

    @staticmethod
    def _deb_control_version(deb):
        proc = subprocess.run(
            ["dpkg-deb", "-f", str(deb), "Version"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False,
        )
        version = proc.stdout.strip()
        if proc.returncode != 0 or not version:
            raise AssertionError(
                f"no control Version in {deb} (dpkg-deb exit {proc.returncode}): "
                f"{proc.stderr[-2000:]}")
        return version

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
        box.assert_reports("reify --version", f"reify {self.version}")
        box.assert_reports("reify-gui --version", f"reify-gui {self.version}")
        box.assert_usage("reify-gui --help")
        box.assert_reports("reify-gui cli --version", f"reify {self.version}")
        box.assert_none_installed(BUNDLED_NOT_DEPENDED)
        box.check(f"reify check {EXAMPLE}")
        box.assert_gui_renders(f"xvfb-run -a reify-gui {EXAMPLE}")
        box.assert_debug_server_reports(self.version)
        self.assertIn("nodejs", box.installed_packages(),
                      "the deb's Depends must bring nodejs onto a stock image")
        box.check("command -v node")
        box.assert_sidecar_ready("/usr/bin/reify-sidecar")
        self.assertIn("python3", box.installed_packages(),
                      "the deb's Depends must bring python3 onto a stock image")
        box.assert_assistant_sandboxed("/usr/bin/reify-sidecar", "/" + LANDLOCK_EXEC)

    def test_appimage(self):
        self.assertIn(f"_{self.version}_", self.appimage.name,
                      "the AppImage must be named for the deb's version")
        appimage = f"/assets/{self.appimage.name}"
        box = self.container((self.appimage, appimage))
        box.assert_stock_premises()
        box.apt_install("xvfb", "xauth", "curl", *DESKTOP_STAND_IN)
        box.assert_none_installed(BUNDLED_NOT_DEPENDED)
        box.assert_none_installed(APPIMAGE_CARRIES_WEBKIT)
        # The extracted tree is for the sidecar and landlock asserts below.
        box.check(f"mkdir -p /tmp/extract && cd /tmp/extract && {appimage} --appimage-extract >/dev/null")
        box.assert_reports(f"APPIMAGE_EXTRACT_AND_RUN=1 {appimage} --version",
                           f"reify-gui {self.version}")
        box.assert_usage(f"APPIMAGE_EXTRACT_AND_RUN=1 {appimage} --help")
        box.assert_reports(f"APPIMAGE_EXTRACT_AND_RUN=1 {appimage} cli --version",
                           f"reify {self.version}")
        box.check(f"cd {EXAMPLE_DIR} && APPIMAGE_EXTRACT_AND_RUN=1 {appimage} cli check {EXAMPLE_NAME}")
        box.assert_gui_renders(
            f"cd {EXAMPLE_DIR} && APPIMAGE_EXTRACT_AND_RUN=1 xvfb-run -a {appimage} {EXAMPLE_NAME}")
        box.assert_debug_server_reports(self.version)
        # An AppImage can declare no Depends, so the user installs node as the refusal says.
        box.check("! command -v node")
        box.apt_install("nodejs")
        box.assert_sidecar_ready("/tmp/extract/squashfs-root/usr/bin/reify-sidecar")
        # An Ubuntu desktop ships python3: the stand-in gains it only here, so nothing above ran with it.
        box.apt_install("python3")
        box.assert_assistant_sandboxed("/tmp/extract/squashfs-root/usr/bin/reify-sidecar",
                                       "/tmp/extract/squashfs-root/" + LANDLOCK_EXEC)


if __name__ == "__main__":
    unittest.main()
