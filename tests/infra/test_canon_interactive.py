#!/usr/bin/env python3
"""
test_canon_interactive.py — stdlib unittest for the canonical-build-path PRD's
interactive half (dark-factory plans/canonical-build-path-shared-cache-prd.md
C7, task λ): scripts/install-canon-trampoline.sh against a scratch HOME shaped
like a stock Ubuntu account with rustup, scripts/setup-dev.sh's wiring of it
and of its own in-tree builds through scripts/canon_run.py, and the GUI
launchers scripts/run-gui*.sh building and launching through canon_run.py
while reify-gui itself sees its real cwd, file and loader path.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_interactive.sh.

The CANON cases enter mount namespaces with /usr/bin/bwrap and SKIP with the
named reason where none can be entered.
"""

import json
import os
import re
import select
import shlex
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

INSTALLER = fx.REPO_ROOT / "scripts" / "install-canon-trampoline.sh"
SETUP_DEV = fx.REPO_ROOT / "scripts" / "setup-dev.sh"
GUI_LAUNCH_SCRIPTS = ("run-gui.sh", "run-gui-dev.sh", "lib_gui_launch.sh", "lib_native_ld_path.sh")
GUI_BINARIES = {"run-gui.sh": "target/release/reify-gui", "run-gui-dev.sh": "target/debug/reify-gui"}
TBB_PIN_DIR = "/opt/reify-deps/tbb-pin"
ENV_GENERATOR = Path("/usr/lib/systemd/user-environment-generators/30-systemd-environment-d-generator")
INSTALLED_BIN = Path(".local", "share", "reify-canon", "bin")
BLOCK_START = "# >>> reify-canon trampoline (scripts/install-canon-trampoline.sh) >>>"
BLOCK_END = "# <<< reify-canon trampoline <<<"
STAMP = ".reify-build-root"

CARGO_ENV = """#!/bin/sh
case ":${PATH}:" in
    *:"$HOME/.cargo/bin":*)
        ;;
    *)
        export PATH="$HOME/.cargo/bin:$PATH"
        ;;
esac
"""
BASHRC = """case $- in
    *i*) ;;
      *) return;;
esac
"""
PROFILE = """if [ -n "$BASH_VERSION" ]; then
    if [ -f "$HOME/.bashrc" ]; then
	. "$HOME/.bashrc"
    fi
fi

if [ -d "$HOME/bin" ] ; then
    PATH="$HOME/bin:$PATH"
fi

if [ -d "$HOME/.local/bin" ] ; then
    PATH="$HOME/.local/bin:$PATH"
fi

. "$HOME/.cargo/env"
"""


def make_home(parent):
    """A HOME whose rustup cargo (a recording fake) is reachable from ~/.local/bin and ~/.cargo/bin."""
    home = Path(parent, "home")
    (home / ".cargo").mkdir(parents=True)
    rustup_bin = fx.fake_cargo_dir(home / ".cargo", "bin")
    (home / ".cargo" / "env").write_text(CARGO_ENV)
    (home / ".local" / "bin").mkdir(parents=True)
    (home / ".local" / "bin" / "cargo").symlink_to(Path(rustup_bin, "cargo"))
    (home / ".bashrc").write_text(BASHRC)
    (home / ".profile").write_text(PROFILE)
    environment_d = home / ".config" / "environment.d"
    environment_d.mkdir(parents=True)
    (environment_d / "995-user-path.conf").write_text(
        f"PATH={home}/.cargo/bin:{home}/.local/bin:{home}/bin:${{PATH}}\n"
    )
    return os.path.realpath(home)


def run_installer(home):
    return fx.run(["/bin/bash", str(INSTALLER)], cwd=home, env={"HOME": home, "PATH": "/usr/bin:/bin"})


def login_shell(home, script, **env):
    """script in a fresh bash login shell that inherits nothing but HOME and env."""
    assignments = [f"{name}={value}" for name, value in {"HOME": home, **env}.items()]
    return fx.run(["/usr/bin/env", "-i", *assignments, "/bin/bash", "-lc", script], cwd=home, env={})


def login_cargo(home):
    result = login_shell(home, "command -v cargo")
    lines = result.stdout.splitlines()
    return lines[-1] if lines else ""


def first_cargo(path_value):
    for entry in path_value.split(":"):
        candidate = os.path.join(entry, "cargo")
        if entry and os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    return None


HEREDOC_START = re.compile(r"(?<!<)<<(?!<)(-?)\s*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\2")
INSTALLER_CALL = re.compile(r'^\s*(if\s+)?"\$\(dirname "\$\{BASH_SOURCE\[0\]\}"\)/install-canon-trampoline\.sh"')
CHECK = r"cargo\s+check\s+--workspace"
RELEASE_BUILD = r"cargo\s+build\s+--release\s+-p\s+reify-cli"
IN_TREE_BUILD = re.compile(rf"{CHECK}|{RELEASE_BUILD}")
ROUTED = {name: re.compile(rf'canon_run\.py"?\s+{build}') for name, build in (("check", CHECK), ("release build", RELEASE_BUILD))}
BARE = {name: re.compile(rf"^\s*{build}") for name, build in (("check", CHECK), ("release build", RELEASE_BUILD))}


def executable_lines(text):
    """text's lines minus full-line comments and heredoc bodies (each body's terminator included)."""
    lines, terminator, strip_tabs = [], None, False
    for line in text.splitlines():
        if terminator is not None:
            if (line.lstrip("\t") if strip_tabs else line) == terminator:
                terminator = None
            continue
        if line.lstrip().startswith("#"):
            continue
        lines.append(line)
        heredoc = HEREDOC_START.search(line)
        if heredoc:
            strip_tabs, terminator = heredoc.group(1) == "-", heredoc.group(3)
    return lines


def wiring_defects(text):
    """What setup-dev.sh text gets wrong about installing the trampoline and routing its builds."""
    lines = executable_lines(text)
    defects = []
    installer = next((index for index, line in enumerate(lines) if INSTALLER_CALL.match(line)), None)
    first_build = next((index for index, line in enumerate(lines) if IN_TREE_BUILD.search(line)), len(lines))
    if installer is None:
        defects.append("installer not invoked")
    elif installer > first_build:
        defects.append("installer invoked after an in-tree build")
    for name, routed in ROUTED.items():
        if not any(routed.search(line) for line in lines):
            defects.append(f"{name} not routed through canon_run.py")
    for name, bare in BARE.items():
        if any(bare.match(line) for line in lines):
            defects.append(f"bare {name}")
    return defects


def mutated(test, text, pattern, replacement):
    """text with every line matching pattern rewritten; the mutation must apply."""
    mutant = re.sub(pattern, replacement, text, flags=re.MULTILINE)
    if mutant == text:
        test.fail(f"mutation {pattern!r} matched nothing in setup-dev.sh")
    return mutant


NPM_STUB = """#!/bin/sh
if [ "$1" = run ] && [ "$2" = dev ]; then exec sleep 600; fi
exit 0
"""
# Exits 7 (connection refused) on its first call and 0 after, so the dev
# launcher's readiness loop takes one retry.
CURL_STUB = """#!/bin/sh
counter="$(dirname "$0")/curl.count"
[ -e "$counter" ] && exit 0
: > "$counter"
exit 7
"""
GUI_DUMP_STUB = """#!/usr/bin/python3 -I
import json, os, sys

def identity(path):
    status = os.stat(path)
    return f"{status.st_dev}:{status.st_ino}"

canon = os.environ.get("REIFY_CANON_ROOT", "")
with open(f"/proc/{os.getppid()}/environ", "rb") as parent:
    parent_names = [entry.partition(b"=")[0] for entry in parent.read().split(b"\\0")]
dump = {
    "argv": sys.argv,
    "cwd": os.getcwd(),
    "canon_identity": identity(canon) if os.path.isdir(canon) else None,
    "canon_active": "REIFY_CANON_ACTIVE" in os.environ,
    "ld_library_path": os.environ.get("LD_LIBRARY_PATH"),
    "parent_has_ld_library_path": b"LD_LIBRARY_PATH" in parent_names,
}
with open(os.environ["TEST_GUI_DUMP"], "w") as out:
    json.dump(dump, out)
"""
# Records its pid, traps TERM (marker, exit 143), then signals readiness on the FIFO.
GUI_TRAP_STUB = """#!/bin/sh
echo $$ > "$TEST_GUI_PIDFILE"
trap 'echo trapped > "$TEST_GUI_MARKER"; kill $! 2>/dev/null; exit 143' TERM
sleep 600 &
echo ready > "$TEST_GUI_FIFO"
wait
"""


def write_executable(path, text):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(0o755)


def make_gui_tree(parent, gui_stub):
    """make_tree plus the GUI launchers, stub sidecar/frontend, an in-tree design file and stub binaries."""
    tree = fx.make_tree(parent, "tree")
    for name in GUI_LAUNCH_SCRIPTS:
        fx.shutil.copy2(fx.REPO_ROOT / "scripts" / name, Path(tree, "scripts", name))
    write_executable(Path(tree, "gui", "sidecar", "build-sidecar.sh"), "#!/bin/sh\nexit 0\n")
    Path(tree, "gui", "package.json").write_text("{}")
    Path(tree, "examples").mkdir()
    Path(tree, "examples", "design.ri").write_text("")
    for binary in GUI_BINARIES.values():
        write_executable(Path(tree, binary), gui_stub)
    return tree


def kill_session(session):
    """SIGKILL every process left in session (a launcher started as its own session's leader)."""
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            if os.getsid(int(entry)) == session:
                os.kill(int(entry), signal.SIGKILL)
        except OSError:
            pass


class InstallerTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-canon-interactive-")
        self.addCleanup(tmp.cleanup)
        self.tmp = os.path.realpath(tmp.name)
        self.home = make_home(self.tmp)
        self.installed = os.path.join(self.home, INSTALLED_BIN, "cargo")
        self.rustup_cargo = os.path.join(self.home, ".cargo", "bin", "cargo")

    def install(self):
        result = run_installer(self.home)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def test_a_installs_a_copy_that_login_shells_resolve(self):
        self.install()
        installed = Path(self.installed)
        self.assertFalse(installed.is_symlink())
        self.assertTrue(installed.is_file())
        self.assertTrue(os.access(installed, os.X_OK))
        self.assertEqual(installed.read_bytes(), fx.TRAMPOLINE.read_bytes())
        self.assertEqual(login_cargo(self.home), self.installed)

    def test_b_a_rerun_repairs_the_copy_and_moves_the_block_last(self):
        self.install()
        Path(self.installed).write_bytes(b"#!/bin/sh\necho stale\n")
        overtaking = fx.fake_cargo_dir(self.tmp, "later-bin")
        profile = Path(self.home, ".profile")
        profile.write_text(profile.read_text() + f'PATH="{overtaking}:$PATH"\n')
        self.assertEqual(login_cargo(self.home), os.path.join(overtaking, "cargo"))

        self.install()
        text = profile.read_text()
        self.assertEqual(text.count(BLOCK_START), 1, text)
        self.assertEqual(text.count(BLOCK_END), 1, text)
        self.assertEqual(Path(self.installed).read_bytes(), fx.TRAMPOLINE.read_bytes())
        self.assertEqual(login_cargo(self.home), self.installed)

    def test_c_reports_a_cargo_that_shadows_the_trampoline(self):
        unshadowed = self.install()
        self.assertNotIn(self.rustup_cargo, unshadowed.stdout + unshadowed.stderr)
        Path(self.home, ".bash_profile").write_text('. "$HOME/.cargo/env"\n')
        self.assertEqual(login_cargo(self.home), self.rustup_cargo)

        shadowed = self.install()
        self.assertIn(self.rustup_cargo, shadowed.stdout + shadowed.stderr)

    def test_d_systemd_user_sessions_resolve_the_trampoline(self):
        if not os.access(ENV_GENERATOR, os.X_OK):
            self.skipTest(f"{ENV_GENERATOR} is absent: no systemd environment.d generator here")
        self.install()
        generated = fx.run(["/usr/bin/env", "-i", f"HOME={self.home}", str(ENV_GENERATOR)], cwd="/", env={})
        self.assertEqual(generated.returncode, 0, generated.stderr)
        paths = [line[len("PATH="):] for line in generated.stdout.splitlines() if line.startswith("PATH=")]
        self.assertEqual(len(paths), 1, generated.stdout)
        self.assertEqual(first_cargo(paths[0]), self.installed, paths[0])

    def test_e_a_login_shell_build_in_a_fresh_tree_runs_at_canon(self):
        reason = fx.namespace_unavailable_reason()
        if reason:
            self.skipTest(reason)
        self.install()
        host_config = os.path.join(self.tmp, "host.toml")
        fx.write_host_config(host_config, enabled=True, policy="incremental")
        canon = os.path.join(self.tmp, "canon")
        os.mkdir(canon)
        tree = fx.make_tree(self.tmp, "tree")
        canon_env = {"REIFY_CANON_ROOT": canon, "REIFY_CANON_HOST_CONFIG": host_config}

        status = fx.run_canon(tree, tree, "status", env={"HOME": self.home, "PATH": "/usr/bin:/bin", **canon_env})
        self.assertEqual(status.returncode, 0, status.stderr)
        report = json.loads(status.stdout)
        self.assertEqual(report["build_dir_mode"], "canon", report["reason"])

        build = login_shell(self.home, f"cd {shlex.quote(tree)} && cargo build", **canon_env)
        self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
        calls = fx.read_calls(os.path.dirname(self.rustup_cargo))
        self.assertEqual([call["cwd"] for call in calls], [canon], calls)
        self.assertEqual(Path(tree, "target", STAMP).read_text().strip(), canon)


class SetupDevWiringTest(unittest.TestCase):
    def setUp(self):
        self.text = SETUP_DEV.read_text()

    def test_detector_ignores_comments_and_heredoc_bodies(self):
        text = (
            '# "$(dirname "${BASH_SOURCE[0]}")/install-canon-trampoline.sh"\n'
            "cat > unit <<-'EOF'\n"
            "\tcargo check --workspace\n"
            "\tEOF\n"
            "echo done\n"
        )
        self.assertEqual(executable_lines(text), ["cat > unit <<-'EOF'", "echo done"])

    def test_setup_dev_installs_the_trampoline_and_routes_its_builds(self):
        self.assertEqual(wiring_defects(self.text), [])

    def test_detector_reports_a_commented_out_installer(self):
        mutant = mutated(self, self.text, r"^(\s*)(.*/install-canon-trampoline\.sh\")", r"\1# \2")
        self.assertIn("installer not invoked", wiring_defects(mutant))

    def test_detector_reports_a_bare_check(self):
        mutant = mutated(self, self.text, r'^(\s*).*canon_run\.py"?\s+(cargo\s+check\s+--workspace)', r"\1\2")
        defects = wiring_defects(mutant)
        self.assertIn("check not routed through canon_run.py", defects)
        self.assertIn("bare check", defects)


class GuiTreeCase(unittest.TestCase):
    """A scratch GUI tree, stub tools and the launchers' environment; gui_stub is the reify-gui stand-in."""

    gui_stub = GUI_DUMP_STUB

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-canon-interactive-gui-")
        self.addCleanup(tmp.cleanup)
        self.tmp = os.path.realpath(tmp.name)
        self.tree = make_gui_tree(self.tmp, self.gui_stub)
        self.design = os.path.join(self.tree, "examples", "design.ri")
        self.stub_bin = fx.fake_cargo_dir(self.tmp, "stub-bin")
        write_executable(Path(self.stub_bin, "npm"), NPM_STUB)
        write_executable(Path(self.stub_bin, "curl"), CURL_STUB)
        snap = Path(self.tmp, "snap-lib")
        snap.mkdir()
        self.expected_loader_path = (f"{TBB_PIN_DIR}:" if os.path.isdir(TBB_PIN_DIR) else "") + str(snap)
        home = Path(self.tmp, "home")
        home.mkdir()
        self.canon = os.path.join(self.tmp, "canon")
        os.mkdir(self.canon)
        self.host_config = os.path.join(self.tmp, "host.toml")
        self.dump = os.path.join(self.tmp, "gui-dump.json")
        self.env = {
            "HOME": str(home),
            "PATH": f"{self.stub_bin}:/usr/bin:/bin",
            "REIFY_GUI_SKIP_PREFLIGHT": "1",
            "REIFY_VITE_PORT": "14200",
            "REIFY_NATIVE_LD_SNAP_DIR": str(snap),
            "REIFY_CANON_ROOT": self.canon,
            "REIFY_CANON_HOST_CONFIG": self.host_config,
            "TEST_GUI_DUMP": self.dump,
        }

    def enable_canon(self):
        reason = fx.namespace_unavailable_reason()
        if reason:
            self.skipTest(reason)
        fx.write_host_config(self.host_config, enabled=True, policy="incremental")

    def start_launcher(self, launcher):
        log_path = Path(self.tmp, f"{launcher}.log")
        with open(log_path, "w") as log:
            process = subprocess.Popen(
                ["/bin/bash", os.path.join(self.tree, "scripts", launcher), self.design],
                cwd=self.tmp,
                env=self.env,
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
        self.addCleanup(process.wait)
        self.addCleanup(kill_session, process.pid)
        return process, log_path

    def launch(self, launcher):
        process, log_path = self.start_launcher(launcher)
        returncode = process.wait()
        self.assertEqual(returncode, 0, log_path.read_text())
        return json.loads(Path(self.dump).read_text())

    def assert_built_at(self, root):
        builds = [call for call in fx.read_calls(self.stub_bin) if call["argv"][1:4] == ["build", "-p", "reify-gui"]]
        self.assertEqual([call["cwd"] for call in builds], [root], builds)

    def assert_gui_saw_real_paths(self, dump):
        self.assertEqual(dump["argv"][1:], [self.design])
        self.assertEqual(dump["cwd"], self.tree)
        self.assertEqual(dump["ld_library_path"], self.expected_loader_path)

    def assert_real_path_launch(self, launcher):
        dump = self.launch(launcher)
        self.assert_built_at(self.tree)
        self.assert_gui_saw_real_paths(dump)
        self.assertFalse(dump["canon_active"])
        self.assertFalse(Path(self.tree, "target", STAMP).exists())

    def assert_canon_launch(self, launcher):
        self.enable_canon()
        dump = self.launch(launcher)
        self.assert_built_at(self.canon)
        self.assertEqual(Path(self.tree, "target", STAMP).read_text().strip(), self.canon)
        tree_status = os.stat(self.tree)
        self.assertTrue(dump["canon_active"], dump)
        self.assertEqual(dump["canon_identity"], f"{tree_status.st_dev}:{tree_status.st_ino}")
        self.assert_gui_saw_real_paths(dump)
        self.assertFalse(dump["parent_has_ld_library_path"], dump)


class GuiLaunchTest(GuiTreeCase):
    def test_a_run_gui_at_the_real_path(self):
        self.assert_real_path_launch("run-gui.sh")

    def test_a_run_gui_dev_at_the_real_path(self):
        self.assert_real_path_launch("run-gui-dev.sh")

    def test_b_run_gui_at_canon(self):
        self.assert_canon_launch("run-gui.sh")

    def test_b_run_gui_dev_at_canon(self):
        self.assert_canon_launch("run-gui-dev.sh")


class GuiTeardownTest(GuiTreeCase):
    """run-gui-dev.sh's TERM cleanup reaches a reify-gui launched at CANON through canon_run."""

    gui_stub = GUI_TRAP_STUB

    def test_c_term_to_run_gui_dev_reaches_the_gui_at_canon(self):
        self.enable_canon()
        fifo = os.path.join(self.tmp, "ready.fifo")
        os.mkfifo(fifo)
        marker = os.path.join(self.tmp, "trapped")
        pidfile = os.path.join(self.tmp, "gui.pid")
        self.env.update(TEST_GUI_FIFO=fifo, TEST_GUI_MARKER=marker, TEST_GUI_PIDFILE=pidfile)
        ready = os.open(fifo, os.O_RDONLY | os.O_NONBLOCK)
        self.addCleanup(os.close, ready)
        process, log_path = self.start_launcher("run-gui-dev.sh")
        launcher_exit = os.pidfd_open(process.pid)
        self.addCleanup(os.close, launcher_exit)

        waiting = select.poll()
        waiting.register(ready, select.POLLIN)
        waiting.register(launcher_exit, select.POLLIN)
        signalled = {fd for fd, _ in waiting.poll()}
        self.assertIn(ready, signalled, f"the launcher exited before the GUI started:\n{log_path.read_text()}")

        gui_pid = int(Path(pidfile).read_text())
        os.kill(process.pid, signal.SIGTERM)
        process.wait()
        self.assertTrue(os.path.exists(marker), log_path.read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(gui_pid, 0)


if __name__ == "__main__":
    unittest.main()
