#!/usr/bin/env python3
"""
test_release_alpha.py — stdlib unittest for scripts/release-alpha.sh, the
unattended release action of the alpha-spine release gates (task 9732,
docs/prds/alpha-spine.md D7 and §5.3).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered
member is the thin wrapper tests/infra/test_release_alpha.sh, which invokes
this file.

Hermetic: each case runs a COPY of the real script inside a throwaway git repo
whose push remote is a local bare repo under a `github.com/` path (so the
script's remote filter matches and the push stays on disk), with a stub
scripts/build-bundle.sh honouring its stdout contract and a stub `gh` first on
PATH that serves auth, repo, release and issue calls from a state dir and logs
every argv. Nothing reaches the network or the real repository.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_SCRIPT = REPO_ROOT / "scripts" / "release-alpha.sh"
BUILD_BUNDLE = REPO_ROOT / "scripts" / "build-bundle.sh"

TAG = "v9.9-test"
NOTE = f"docs/releases/{TAG}.md"
RELEASE_URL = f"https://github.com/fake/repo/releases/tag/{TAG}"

GH_STUB = r'''#!@PYTHON@
import json
import os
import sys
from pathlib import Path

state = Path(os.environ["GH_STUB_STATE"])
args = sys.argv[1:]
with open(state / "calls.jsonl", "a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\n")


def flag(name):
    return args[args.index(name) + 1] if name in args else None


group, verb = (args + ["", ""])[:2]
if (group, verb) == ("auth", "status"):
    sys.exit(1 if (state / "unauthenticated").exists() else 0)
if (group, verb) == ("repo", "view"):
    print("fake/repo")
    sys.exit(0)
if group == "release":
    release = state / "releases" / args[2]
    if verb == "view":
        if not release.exists():
            sys.exit(1)
        if flag("--json") == "url":
            print("https://github.com/fake/repo/releases/tag/" + args[2])
        sys.exit(0)
    if verb == "create":
        release.parent.mkdir(exist_ok=True)
        release.write_text("", encoding="utf-8")
        sys.exit(0)
    if verb in ("upload", "edit"):
        sys.exit(0 if release.exists() else 1)
if group == "issue":
    issue = state / "issues" / args[2]
    if verb == "view":
        if not issue.exists():
            sys.exit(1)
        print(issue.read_text(encoding="utf-8").strip())
        sys.exit(0)
    if verb == "close":
        issue.write_text("CLOSED\n", encoding="utf-8")
        sys.exit(0)
sys.exit(99)
'''

BUILD_BUNDLE_STUB = '''#!/usr/bin/env bash
echo "build-bundle (stub): building" >&2
printf '%s\\n' "@DEB@" "@APPIMAGE@"
'''


def _hermetic_env(root):
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update({
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_NAME": "release-test",
        "GIT_AUTHOR_EMAIL": "release-test@example.invalid",
        "GIT_COMMITTER_NAME": "release-test",
        "GIT_COMMITTER_EMAIL": "release-test@example.invalid",
        "HOME": str(root / "home"),
        "PATH": f"{root / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}",
        "GH_STUB_STATE": str(root / "gh-state"),
    })
    return env


class ReleaseFixture:
    """A main-branch repo holding a copy of the real release script, a stub
    build-bundle.sh and the note; issues 1 and 2 open; no release yet."""

    def __init__(self, root):
        self.root = root
        self.repo = root / "repo"
        self.remote = root / "github.com" / "fake" / "repo.git"
        self.state = root / "gh-state"
        self.deb = root / "assets" / "reify_0.1.0_amd64.deb"
        self.appimage = root / "assets" / "Reify_0.1.0_amd64.AppImage"
        self.env = _hermetic_env(root)
        for d in (root / "home", root / "bin", self.state / "issues", self.deb.parent):
            d.mkdir(parents=True)
        self.deb.write_bytes(b"deb")
        self.appimage.write_bytes(b"appimage")
        for n in ("1", "2"):
            (self.state / "issues" / n).write_text("OPEN\n", encoding="utf-8")
        gh = root / "bin" / "gh"
        gh.write_text(GH_STUB.replace("@PYTHON@", sys.executable), encoding="utf-8")
        gh.chmod(0o755)
        self._init_repo()

    def _init_repo(self):
        self.git("init", "-q", "--bare", "-b", "main", str(self.remote), cwd=self.root)
        self.git("init", "-q", "-b", "main", str(self.repo), cwd=self.root)
        scripts = self.repo / "scripts"
        scripts.mkdir()
        shutil.copy2(RELEASE_SCRIPT, scripts / "release-alpha.sh")
        stub = scripts / "build-bundle.sh"
        stub.write_text(BUILD_BUNDLE_STUB.replace("@DEB@", str(self.deb))
                        .replace("@APPIMAGE@", str(self.appimage)), encoding="utf-8")
        stub.chmod(0o755)
        note = self.repo / NOTE
        note.parent.mkdir(parents=True)
        note.write_text(f"# {TAG}\n\nCloses: #1, #2\n", encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "fixture")
        self.git("remote", "add", "origin", str(self.remote))

    def git(self, *args, cwd=None):
        proc = subprocess.run(
            ["git", *args], cwd=cwd or self.repo, env=self.env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False,
        )
        if proc.returncode != 0:
            raise AssertionError(f"git {args} failed:\n{proc.stderr}")
        return proc.stdout.strip()

    def run_release(self):
        return subprocess.run(
            ["bash", str(self.repo / "scripts" / "release-alpha.sh"), TAG],
            cwd=self.repo, env=self.env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False,
        )

    def gh_calls(self):
        log = self.state / "calls.jsonl"
        if not log.exists():
            return []
        return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]

    def clear_gh_calls(self):
        (self.state / "calls.jsonl").unlink(missing_ok=True)

    def remote_tag_type(self):
        proc = subprocess.run(
            ["git", "--git-dir", str(self.remote), "cat-file", "-t", f"refs/tags/{TAG}"],
            env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            check=False,
        )
        return proc.stdout.strip() if proc.returncode == 0 else None


def _verb_calls(calls, group, verb):
    return [c for c in calls if c[:2] == [group, verb]]


class ReleaseCase(unittest.TestCase):

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.fx = ReleaseFixture(Path(tmp.name))

    def assert_refused(self, proc, message):
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn(message, proc.stderr)
        self.assertEqual(_verb_calls(self.fx.gh_calls(), "release", "create"), [])
        self.assertEqual(self.fx.git("tag", "-l"), "")


class Preflight(ReleaseCase):

    def test_a_non_main_head_is_refused(self):
        self.fx.git("checkout", "-q", "-b", "feature")
        self.assert_refused(self.fx.run_release(), "HEAD is not main")

    def test_a_missing_note_is_refused(self):
        (self.fx.repo / NOTE).unlink()
        self.assert_refused(self.fx.run_release(), "missing migration note")

    def test_an_absent_build_bundle_is_refused(self):
        (self.fx.repo / "scripts" / "build-bundle.sh").unlink()
        self.assert_refused(
            self.fx.run_release(), "build-bundle.sh missing or not executable")

    def test_a_non_executable_build_bundle_is_refused(self):
        (self.fx.repo / "scripts" / "build-bundle.sh").chmod(0o644)
        self.assert_refused(
            self.fx.run_release(), "build-bundle.sh missing or not executable")

    def test_an_unauthenticated_gh_is_refused(self):
        (self.fx.state / "unauthenticated").write_text("", encoding="utf-8")
        self.assert_refused(self.fx.run_release(), "not authenticated")


class Release(ReleaseCase):

    def setUp(self):
        super().setUp()
        self.first = self.fx.run_release()
        self.head = self.fx.git("rev-parse", "HEAD")

    def test_first_run_succeeds(self):
        self.assertEqual(self.first.returncode, 0, self.first.stderr)

    def test_an_annotated_tag_at_head_reaches_the_remote(self):
        self.assertEqual(self.fx.remote_tag_type(), "tag")
        self.assertEqual(
            self.fx.git("--git-dir", str(self.fx.remote), "rev-parse", f"{TAG}^{{commit}}"),
            self.head)

    def test_the_prerelease_carries_both_assets_and_the_note(self):
        (create,) = _verb_calls(self.fx.gh_calls(), "release", "create")
        self.assertEqual(create[:5], ["release", "create", TAG,
                                      str(self.fx.deb), str(self.fx.appimage)])
        self.assertEqual(create[create.index("--notes-file") + 1], NOTE)
        self.assertIn("--prerelease", create)

    def test_issues_named_by_the_note_are_closed_with_the_release_url(self):
        closes = _verb_calls(self.fx.gh_calls(), "issue", "close")
        self.assertEqual(sorted(c[2] for c in closes), ["1", "2"])
        for close in closes:
            self.assertIn(RELEASE_URL, close[close.index("--comment") + 1])

    def test_stdout_ends_with_the_asset_paths(self):
        self.assertEqual(self.first.stdout.splitlines()[-2:],
                         [str(self.fx.deb), str(self.fx.appimage)])

    def test_a_rerun_reuploads_instead_of_recreating(self):
        self.fx.clear_gh_calls()
        second = self.fx.run_release()
        self.assertEqual(second.returncode, 0, second.stderr)
        calls = self.fx.gh_calls()
        self.assertEqual(_verb_calls(calls, "release", "create"), [])
        (upload,) = _verb_calls(calls, "release", "upload")
        self.assertIn("--clobber", upload)
        self.assertEqual(len(_verb_calls(calls, "release", "edit")), 1)
        self.assertIn("already closed #1", second.stdout)
        self.assertIn("already closed #2", second.stdout)
        self.assertEqual(_verb_calls(calls, "issue", "close"), [])


class RealTree(unittest.TestCase):
    """The release gates (#9735, #9738) exec these two scripts directly."""

    def assert_executable_bash(self, path):
        self.assertTrue(path.is_file(), f"{path} does not exist")
        self.assertTrue(os.access(path, os.X_OK), f"{path} is not executable")
        proc = subprocess.run(
            ["bash", "-n", str(path)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_release_alpha_is_executable_bash(self):
        self.assert_executable_bash(RELEASE_SCRIPT)

    def test_build_bundle_is_executable_bash(self):
        self.assert_executable_bash(BUILD_BUNDLE)


if __name__ == "__main__":
    unittest.main()
