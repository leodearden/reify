#!/usr/bin/env python3
"""
test_canon_interactive.py — stdlib unittest for the canonical-build-path PRD's
interactive half (dark-factory plans/canonical-build-path-shared-cache-prd.md
C7, task λ): scripts/install-canon-trampoline.sh against a scratch HOME shaped
like a stock Ubuntu account with rustup, and scripts/setup-dev.sh's wiring of
it and of its own in-tree builds through scripts/canon_run.py.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_interactive.sh.

The CANON cases enter mount namespaces with /usr/bin/bwrap and SKIP with the
named reason where none can be entered.
"""

import json
import os
import re
import shlex
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

INSTALLER = fx.REPO_ROOT / "scripts" / "install-canon-trampoline.sh"
SETUP_DEV = fx.REPO_ROOT / "scripts" / "setup-dev.sh"
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
        mutant = mutated(self, self.text, r'^(\s*)\S*canon_run\.py"?\s+(cargo\s+check\s+--workspace)', r"\1\2")
        defects = wiring_defects(mutant)
        self.assertIn("check not routed through canon_run.py", defects)
        self.assertIn("bare check", defects)


if __name__ == "__main__":
    unittest.main()
