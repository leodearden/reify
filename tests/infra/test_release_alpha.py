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
scripts/build-bundle.sh honouring its stdout contract, a stub
tests/infra/test_bundle_clean_install.py that records its argv and proof knobs
and exits with a fixture-controlled code, and a stub `gh` first on PATH that
serves auth, repo, release and issue calls from a state dir and logs every
argv. Nothing reaches the network or the real repository.

The B7 proof gate (task 10079): the release runs the clean-container proof
armed over exactly the one .deb and one .AppImage it publishes, and a red proof
leaves no tag, release or closed issue, on a re-run too. ProofArmChain drives
the REAL proof with the same arm env and a failing stub docker, so a renamed arm
variable reds instead of shipping behind an all-skipped green, and a docker that
answers `info` but cannot create a container fails the armed proof before any
container (task 10181).

The tag invariant (task 10104): a tag that already exists must name the HEAD the
release built and proved, in preflight and again just before the push. TagAtPush
moves the tag during the build, a window only the second check can see.

The version handoff (task 10021): the release builds its assets at the
version its tag names. Release and Preflight pin the release's end;
BuildBundleVersion pins build-bundle.sh's, running a copy of the real script
with every tool it runs stubbed: the `--version` usage refusal, the version
overlay on both `cargo tauri` calls, and the deb control-Version post-check.
Since task 10485 it also pins the binaries' end: both cargo builds see the
version as REIFY_RELEASE_VERSION, a built binary whose `--version` reports
another one is refused before bundling, and with no `--version` the version
is Cargo.toml's workspace version, whatever REIFY_RELEASE_VERSION the caller
carries.
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
DOCKER_CREATE_PROBE = REPO_ROOT / "scripts" / "docker-create-probe.sh"
PROOF = "tests/infra/test_bundle_clean_install.py"

PROOF_ARM_ENV = {"REIFY_RUN_BUNDLE_CLEAN_INSTALL": "1"}
PROOF_KNOBS = ("REIFY_RUN_BUNDLE_CLEAN_INSTALL", "REIFY_BUNDLE_DEB", "REIFY_BUNDLE_APPIMAGE")

TAG = "v9.9-test"
BUNDLE_VERSION = "9.9.0-test"
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

BUILD_BUNDLE_BANNER = "build-bundle (stub): building"
BUILD_BUNDLE_STUB = '''#!/usr/bin/env bash
for arg in "$@"; do printf '%s\\0' "$arg"; done > "@ARGV_LOG@"
echo "@BANNER@" >&2
printf '%s\\n' "@DEB@" "@APPIMAGE@"
'''.replace("@BANNER@", BUILD_BUNDLE_BANNER)

PROOF_STUB = r'''#!@PYTHON@
import json
import os
import sys
from pathlib import Path

state = Path("@STATE@")
with open(state / "calls.jsonl", "a", encoding="utf-8") as log:
    log.write(json.dumps({"argv": sys.argv[1:],
                          "env": {k: os.environ.get(k) for k in @KNOBS@}}) + "\n")
print("clean-install proof (stub): ran", file=sys.stderr)
exit_file = state / "exit"
sys.exit(int(exit_file.read_text(encoding="utf-8")) if exit_file.exists() else 0)
'''

DOCKER_STUB = '''#!/usr/bin/env bash
printf '%s\\n' "$*" >> "@LOG@"
exit 1
'''

DOCKER_CREATE_HANGS_STUB = '''#!/usr/bin/env bash
printf '%s\\n' "$*" >> "@LOG@"
case "$1" in info) exit 0 ;; create) exec sleep 60 ;; *) exit 1 ;; esac
'''

BUILD_BUNDLE_USAGE = "usage: scripts/build-bundle.sh [--version <semver>]"
WORKSPACE_VERSION = "4.5.6"

# One stub for every tool build-bundle.sh runs; it dispatches on the name it was
# invoked under and logs [name, *argv]. The tauri calls leave the files the
# script checks for, bundle_closure.py stages one library and names it in the
# overlay, dpkg-deb extracts the members the post-check requires and reports
# the control Version the fixture chose, and docker create hangs when the
# fixture asks. Like reify_version::VERSION, each built binary's `--version`
# reports the REIFY_RELEASE_VERSION its cargo call saw when that is non-empty,
# else the workspace version, unless the fixture forces what it reports.
BUNDLE_TOOL_STUB = r'''#!@PYTHON@
import json
import os
import shlex
import shutil
import sys
import time
from pathlib import Path

name = Path(sys.argv[0]).name
args = sys.argv[1:]
state = Path("@STATE@")
target = Path("@TARGET@")
with open(state / "calls.jsonl", "a", encoding="utf-8") as log:
    log.write(json.dumps([name, *args]) + "\n")


def flag(option):
    return args[args.index(option) + 1]


def make(path, data=b"stub", mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    path.chmod(mode)


def build_binary(binary):
    forced = state / "binaries-report"
    if forced.exists():
        stamp = forced.read_text(encoding="utf-8").strip()
    else:
        stamp = os.environ.get("REIFY_RELEASE_VERSION") or "@WORKSPACE_VERSION@"
    line = shlex.quote(f"{binary} {stamp}")
    script = f'#!/bin/sh\ncase "$1" in --version|-V) echo {line} ;; *) exit 64 ;; esac\n'
    make(target / "release" / binary, script.encode(), mode=0o755)


if name == "cargo":
    if args[:2] == ["tauri", "build"]:
        build_binary("reify-gui")
    elif args[:1] == ["build"]:
        build_binary("reify")
    elif args[:1] == ["metadata"]:
        print(json.dumps({"target_directory": str(target)}))
    elif args[:2] == ["tauri", "bundle"]:
        make(target / "release" / "bundle" / "deb" / "Reify_amd64.deb")
        make(target / "release" / "bundle" / "appimage" / "Reify_amd64.AppImage")
elif name == "docker" and args[:1] == ["create"] and (state / "docker-create-hangs").exists():
    time.sleep(60)
elif name == "bundle_closure.py":
    libs = Path(flag("--stage")) / "lib"
    make(libs / "libstub.so.1", b"staged library")
    Path(flag("--overlay")).write_text(json.dumps(
        {"bundle": {"linux": {"deb": {"files": {"/usr/lib/Reify/lib": str(libs)}}}}}))
elif name == "dpkg-deb":
    if args[0] == "--extract":
        root = Path(args[2])
        for member in ("reify-gui", "reify", "reify-sidecar"):
            make(root / "usr" / "bin" / member)
        shutil.copytree(target / "bundle-stage" / "lib", root / "usr" / "lib" / "Reify" / "lib")
    elif args[0] == "-f":
        print((state / "deb-version").read_text(encoding="utf-8").strip())
'''


def _hermetic_env(root):
    env = {k: v for k, v in os.environ.items()
           if not k.startswith("GIT_") and k not in PROOF_KNOBS}
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
        self.proof_state = root / "proof-state"
        self.deb = root / "assets" / f"Reify_{BUNDLE_VERSION}_amd64.deb"
        self.appimage = root / "assets" / f"Reify_{BUNDLE_VERSION}_amd64.AppImage"
        self.build_bundle_argv_log = root / "build-bundle-argv"
        self.env = _hermetic_env(root)
        for d in (root / "home", root / "bin", self.state / "issues", self.deb.parent,
                  self.proof_state):
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
        self.write_build_bundle_stub(self.deb, self.appimage)
        proof = self.repo / PROOF
        proof.parent.mkdir(parents=True)
        proof.write_text(PROOF_STUB.replace("@PYTHON@", sys.executable)
                         .replace("@STATE@", str(self.proof_state))
                         .replace("@KNOBS@", json.dumps(PROOF_KNOBS)), encoding="utf-8")
        self.write_note(TAG)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "fixture")
        self.git("remote", "add", "origin", str(self.remote))

    def write_build_bundle_stub(self, deb, appimage):
        """A stub build-bundle.sh that records its argv and prints these assets."""
        stub = self.repo / "scripts" / "build-bundle.sh"
        stub.write_text(BUILD_BUNDLE_STUB.replace("@DEB@", str(deb))
                        .replace("@APPIMAGE@", str(appimage))
                        .replace("@ARGV_LOG@", str(self.build_bundle_argv_log)),
                        encoding="utf-8")
        stub.chmod(0o755)

    def use_assets(self, deb_name, appimage_name):
        """Make the stub build print non-empty assets with these file names."""
        deb, appimage = self.deb.parent / deb_name, self.deb.parent / appimage_name
        deb.write_bytes(b"deb")
        appimage.write_bytes(b"appimage")
        self.write_build_bundle_stub(deb, appimage)
        return deb, appimage

    def write_note(self, tag):
        note = self.repo / "docs" / "releases" / f"{tag}.md"
        note.parent.mkdir(parents=True, exist_ok=True)
        note.write_text(f"# {tag}\n\nCloses: #1, #2\n", encoding="utf-8")

    def build_bundle_argv(self):
        """The argv build-bundle.sh last ran with, or None if it never ran."""
        if not self.build_bundle_argv_log.exists():
            return None
        return self.build_bundle_argv_log.read_bytes().decode("utf-8").split("\0")[:-1]

    def git(self, *args, cwd=None):
        proc = subprocess.run(
            ["git", *args], cwd=cwd or self.repo, env=self.env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False,
        )
        if proc.returncode != 0:
            raise AssertionError(f"git {args} failed:\n{proc.stderr}")
        return proc.stdout.strip()

    def run_release(self, tag=TAG):
        return subprocess.run(
            ["bash", str(self.repo / "scripts" / "release-alpha.sh"), tag],
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

    def proof_calls(self):
        log = self.proof_state / "calls.jsonl"
        if not log.exists():
            return []
        return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]

    def fail_proof(self, code=1):
        (self.proof_state / "exit").write_text(f"{code}\n", encoding="utf-8")

    def issue_state(self, n):
        return (self.state / "issues" / str(n)).read_text(encoding="utf-8").strip()

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

    def test_an_absent_clean_install_proof_is_refused(self):
        (self.fx.repo / PROOF).unlink()
        self.assert_refused(self.fx.run_release(), "test_bundle_clean_install.py missing")
        self.assertEqual(self.fx.proof_calls(), [])

    def assert_tag_refused_before_the_build_and_the_proof(self, proc):
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn(f"tag {TAG} exists and does not point at HEAD", proc.stderr)
        self.assertNotIn(BUILD_BUNDLE_BANNER, proc.stderr)
        self.assertEqual(self.fx.proof_calls(), [])
        self.assertIsNone(self.fx.remote_tag_type())
        self.assertEqual([c for c in self.fx.gh_calls() if c[:1] == ["release"]], [])

    def test_a_tag_not_at_head_is_refused_before_the_build_and_the_proof(self):
        self.fx.git("tag", "-a", TAG, "-m", TAG)
        self.fx.git("commit", "-q", "--allow-empty", "-m", "after the tag")
        self.assert_tag_refused_before_the_build_and_the_proof(self.fx.run_release())

    def test_a_tag_that_names_no_commit_is_refused_before_the_build_and_the_proof(self):
        note_blob = self.fx.git("rev-parse", f"HEAD:{NOTE}")
        self.fx.git("tag", "-a", TAG, "-m", TAG, note_blob)
        self.assert_tag_refused_before_the_build_and_the_proof(self.fx.run_release())


    def test_a_tag_that_names_no_version_is_refused_before_the_build(self):
        for tag in ("latest", "v1", "0.2-alpha", "v01.2", "v0.2-", "v0.2-alpha.",
                    "v0.2-alpha..1", "v0.2-alpha.01"):
            with self.subTest(tag=tag):
                self.fx.write_note(tag)
                proc = self.fx.run_release(tag)
                self.assert_refused(proc, f"tag {tag} does not name a version")
                self.assertNotIn(BUILD_BUNDLE_BANNER, proc.stderr)
                self.assertIsNone(self.fx.build_bundle_argv())
                self.assertEqual(self.fx.proof_calls(), [])
                self.assertEqual([c for c in self.fx.gh_calls() if c[:1] == ["release"]], [])

    def test_a_full_patch_tag_builds_that_version(self):
        self.fx.write_note("v1.2.3")
        self.fx.use_assets("Reify_1.2.3_amd64.deb", "Reify_1.2.3_amd64.AppImage")
        proc = self.fx.run_release("v1.2.3")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(self.fx.build_bundle_argv(), ["--version", "1.2.3"])


class Release(ReleaseCase):

    def setUp(self):
        super().setUp()
        self.first = self.fx.run_release()
        self.head = self.fx.git("rev-parse", "HEAD")

    def test_first_run_succeeds(self):
        self.assertEqual(self.first.returncode, 0, self.first.stderr)

    def test_the_bundle_is_built_at_the_tag_version(self):
        self.assertEqual(self.fx.build_bundle_argv(), ["--version", BUNDLE_VERSION])

    def test_the_proof_runs_once_armed_over_the_built_assets(self):
        (call,) = self.fx.proof_calls()
        self.assertEqual(call["env"], {**PROOF_ARM_ENV,
                                       "REIFY_BUNDLE_DEB": str(self.fx.deb),
                                       "REIFY_BUNDLE_APPIMAGE": str(self.fx.appimage)})

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


class ProofGate(ReleaseCase):
    """B7 (docs/prds/alpha-spine.md §6) gates every tag, push, release and
    issue close, on a first run and on a re-run alike."""

    PROOF_FAILED = "B7 clean-container proof failed"

    def test_a_red_proof_leaves_no_tag_release_or_closed_issue(self):
        self.fx.fail_proof()
        self.assert_refused(self.fx.run_release(), self.PROOF_FAILED)
        self.assertEqual(len(self.fx.proof_calls()), 1)
        self.assertIsNone(self.fx.remote_tag_type())
        calls = self.fx.gh_calls()
        self.assertEqual([c for c in calls if c[:1] == ["release"]], [])
        self.assertEqual(_verb_calls(calls, "issue", "close"), [])
        self.assertEqual([self.fx.issue_state(n) for n in (1, 2)], ["OPEN", "OPEN"])

    def test_a_red_proof_on_a_rerun_uploads_nothing(self):
        first = self.fx.run_release()
        self.assertEqual(first.returncode, 0, first.stderr)
        self.fx.clear_gh_calls()
        self.fx.fail_proof()
        second = self.fx.run_release()
        self.assertEqual(second.returncode, 2, second.stderr)
        self.assertIn(self.PROOF_FAILED, second.stderr)
        calls = self.fx.gh_calls()
        for group, verb in (("release", "upload"), ("release", "edit"), ("issue", "close")):
            self.assertEqual(_verb_calls(calls, group, verb), [], f"gh {group} {verb}")

    def test_a_bundle_that_is_not_one_deb_and_one_appimage_is_refused(self):
        self.fx.write_build_bundle_stub(self.fx.deb, self.fx.deb)
        self.assert_refused(self.fx.run_release(), "expected one .deb and one .AppImage")
        self.assertEqual(self.fx.proof_calls(), [])

    def test_assets_not_named_for_the_tag_version_are_refused_before_the_proof(self):
        self.fx.use_assets("Reify_0.1.0_amd64.deb", "Reify_0.1.0_amd64.AppImage")
        self.assert_refused(self.fx.run_release(),
                            f"is not named for version {BUNDLE_VERSION}")
        self.assertEqual(self.fx.proof_calls(), [])
        self.assertIsNone(self.fx.remote_tag_type())


class BuildBundleFixture:
    """A copy of the real build-bundle.sh in a scratch tree where every tool it
    runs is BUNDLE_TOOL_STUB: cargo in $HOME/.cargo/bin, and docker, npm and
    dpkg-deb in $HOME/.local/bin, both of which the script's own PATH prefix
    puts first; the tree's check-manifold-deps.sh, build-sidecar.sh and
    bundle_closure.py run it too, and the tree also carries the real
    docker-create-probe.sh."""

    def __init__(self, root):
        self.repo = root / "repo"
        self.state = root / "tool-state"
        self.target = root / "target"
        tool = root / "tools" / "bundle-tool"
        home = root / "home"
        for d in (self.state, tool.parent, home / ".cargo" / "bin", home / ".local" / "bin",
                  root / "tmp", self.repo / "scripts", self.repo / "gui" / "sidecar",
                  self.repo / "gui" / "src-tauri"):
            d.mkdir(parents=True)
        tool.write_text(BUNDLE_TOOL_STUB.replace("@PYTHON@", sys.executable)
                        .replace("@STATE@", str(self.state))
                        .replace("@TARGET@", str(self.target))
                        .replace("@WORKSPACE_VERSION@", WORKSPACE_VERSION), encoding="utf-8")
        tool.chmod(0o755)
        for link in (home / ".cargo" / "bin" / "cargo",
                     *(home / ".local" / "bin" / n for n in ("docker", "npm", "dpkg-deb")),
                     self.repo / "scripts" / "bundle_closure.py"):
            link.symlink_to(tool)
        for script in ("scripts/check-manifold-deps.sh", "gui/sidecar/build-sidecar.sh"):
            name = script.rsplit("/", 1)[1]
            (tool.parent / name).symlink_to(tool)
            (self.repo / script).write_text(
                f'exec "{tool.parent / name}" "$@"\n', encoding="utf-8")
        shutil.copy2(BUILD_BUNDLE, self.repo / "scripts" / "build-bundle.sh")
        shutil.copy2(DOCKER_CREATE_PROBE, self.repo / "scripts" / "docker-create-probe.sh")
        (self.repo / "Cargo.toml").write_text(
            f'[workspace.package]\nversion = "{WORKSPACE_VERSION}"\n', encoding="utf-8")
        (self.repo / "gui" / "src-tauri" / "tauri.conf.json").write_text(
            json.dumps({}), encoding="utf-8")
        self.env = {k: v for k, v in os.environ.items() if k != "REIFY_RELEASE_VERSION"}
        self.env.update({"HOME": str(home), "TMPDIR": str(root / "tmp")})

    def deb_reports_version(self, version):
        (self.state / "deb-version").write_text(version + "\n", encoding="utf-8")

    def binaries_report(self, version):
        (self.state / "binaries-report").write_text(version + "\n", encoding="utf-8")

    def reported_versions(self):
        """What the built target/release/{reify,reify-gui} print for --version."""
        return {binary: subprocess.run(
                    [str(self.target / "release" / binary), "--version"],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True,
                ).stdout
                for binary in ("reify", "reify-gui")}

    def docker_create_hangs(self):
        (self.state / "docker-create-hangs").write_text("", encoding="utf-8")
        self.env["REIFY_DOCKER_CREATE_PROBE_SECS"] = "5"

    def run(self, *argv):
        return subprocess.run(
            ["bash", str(self.repo / "scripts" / "build-bundle.sh"), *argv],
            cwd=self.repo, env=self.env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False,
        )

    def calls(self):
        log = self.state / "calls.jsonl"
        if not log.exists():
            return []
        return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]

    def tauri_call(self, verb):
        (call,) = [c for c in self.calls() if c[:3] == ["cargo", "tauri", verb]]
        return call

    def assets(self):
        bundle = self.target / "release" / "bundle"
        return [str(bundle / "deb" / "Reify_amd64.deb"),
                str(bundle / "appimage" / "Reify_amd64.AppImage")]


def _inline_configs(call):
    """The JSON objects a cargo tauri call passes inline via --config; a value
    not starting with `{` is a config file path."""
    values = [value for option, value in zip(call, call[1:]) if option == "--config"]
    return [json.loads(value) for value in values if value.startswith("{")]


class BuildBundleVersion(unittest.TestCase):

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.fx = BuildBundleFixture(Path(tmp.name))

    def test_a_bad_argv_is_a_usage_error_before_any_step(self):
        for argv in (["--bogus"], ["--version"], ["--version", ""],
                     ["--version", "1.0.0", "extra"], ["-v", "1.0.0"]):
            with self.subTest(argv=argv):
                proc = self.fx.run(*argv)
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertIn(BUILD_BUNDLE_USAGE, proc.stderr)
                self.assertEqual(proc.stdout, "")
                self.assertEqual(self.fx.calls(), [])

    def test_the_version_overlays_both_tauri_calls(self):
        self.fx.deb_reports_version(BUNDLE_VERSION)
        proc = self.fx.run("--version", BUNDLE_VERSION)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        for verb in ("build", "bundle"):
            with self.subTest(verb=verb):
                self.assertEqual(_inline_configs(self.fx.tauri_call(verb)),
                                 [{"version": BUNDLE_VERSION}])
        self.assertEqual(proc.stdout.splitlines(), self.fx.assets())

    def test_a_deb_of_another_version_is_refused_naming_both(self):
        self.fx.deb_reports_version("0.1.0")
        proc = self.fx.run("--version", BUNDLE_VERSION)
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn(f"has control Version 0.1.0; expected {BUNDLE_VERSION}", proc.stderr)
        self.assertEqual(proc.stdout, "")

    def test_the_version_is_stamped_into_both_binaries(self):
        self.fx.deb_reports_version(BUNDLE_VERSION)
        proc = self.fx.run("--version", BUNDLE_VERSION)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(self.fx.reported_versions(),
                         {"reify": f"reify {BUNDLE_VERSION}\n",
                          "reify-gui": f"reify-gui {BUNDLE_VERSION}\n"})

    def test_a_binary_reporting_another_version_is_refused_before_bundling(self):
        self.fx.deb_reports_version(BUNDLE_VERSION)
        self.fx.binaries_report("0.1.0")
        proc = self.fx.run("--version", BUNDLE_VERSION)
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("'reify-gui 0.1.0'", proc.stderr)
        self.assertIn(f"'reify-gui {BUNDLE_VERSION}'", proc.stderr)
        self.assertEqual(proc.stdout, "")
        calls = self.fx.calls()
        self.assertEqual([c for c in calls if c[:3] == ["cargo", "tauri", "bundle"]], [])
        self.assertEqual([c for c in calls if c[0] == "bundle_closure.py"], [])

    def test_without_a_version_the_workspace_version_names_it(self):
        self.fx.deb_reports_version(WORKSPACE_VERSION)
        proc = self.fx.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        for verb in ("build", "bundle"):
            with self.subTest(verb=verb):
                self.assertEqual(_inline_configs(self.fx.tauri_call(verb)),
                                 [{"version": WORKSPACE_VERSION}])
        self.assertEqual(self.fx.reported_versions(),
                         {"reify": f"reify {WORKSPACE_VERSION}\n",
                          "reify-gui": f"reify-gui {WORKSPACE_VERSION}\n"})
        self.fx.deb_reports_version(BUNDLE_VERSION)
        refused = self.fx.run()
        self.assertEqual(refused.returncode, 2, refused.stderr)
        self.assertIn(f"expected {WORKSPACE_VERSION}", refused.stderr)

    def test_a_callers_release_version_does_not_reach_a_build_without_one(self):
        self.fx.deb_reports_version(WORKSPACE_VERSION)
        self.fx.env["REIFY_RELEASE_VERSION"] = "7.7.7"
        proc = self.fx.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(self.fx.reported_versions(),
                         {"reify": f"reify {WORKSPACE_VERSION}\n",
                          "reify-gui": f"reify-gui {WORKSPACE_VERSION}\n"})


class BuildBundleDockerPreflight(unittest.TestCase):
    """A docker that answers `info` but cannot create a container is refused
    in preflight, before the cold build (task 10181)."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.fx = BuildBundleFixture(Path(tmp.name))

    def test_a_stalled_docker_create_refuses_before_any_build_step(self):
        self.fx.docker_create_hangs()
        proc = self.fx.run()
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertEqual(proc.stdout, "")
        probe_verdicts = [line for line in proc.stderr.splitlines()
                          if line.startswith("docker-create-probe:")]
        self.assertTrue(any("docker create" in line for line in probe_verdicts), proc.stderr)
        self.assertIn("docker preflight failed", proc.stderr)
        calls = self.fx.calls()
        self.assertEqual([c[1] for c in calls if c[0] == "docker"], ["info", "create"])
        self.assertEqual([c for c in calls if c[0] == "npm"], [])
        self.assertEqual([c for c in calls if c[:3] == ["cargo", "tauri", "build"]
                          or c[:2] == ["cargo", "build"]], [])


class TagAtPush(ReleaseCase):
    """The tag pushed and released is re-checked against the HEAD that was
    built and proved immediately before the push, not only in preflight."""

    def test_a_tag_moved_off_head_during_the_build_is_refused_before_the_push(self):
        moved_to = self.fx.git("rev-parse", "HEAD")
        self.fx.git("commit", "-q", "--allow-empty", "-m", "built and proved")
        stub = self.fx.repo / "scripts" / "build-bundle.sh"
        tag_during_build = f"git tag -a {TAG} -m {TAG} {moved_to}\n"
        stub.write_text(stub.read_text(encoding="utf-8") + tag_during_build, encoding="utf-8")
        proc = self.fx.run_release()
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn(f"tag {TAG} exists and does not point at HEAD", proc.stderr)
        # One proof call: preflight passed and the window was crossed, so the check
        # just before the push is what refused.
        self.assertEqual(len(self.fx.proof_calls()), 1)
        self.assertIsNone(self.fx.remote_tag_type())
        self.assertEqual([c for c in self.fx.gh_calls() if c[:1] == ["release"]], [])


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


class ProofArmChain(unittest.TestCase):
    """The env the release sets arms the real B7 proof, not an all-skipped green."""

    def _proof_runner(self, docker_stub):
        """A runner of the real proof over dummy reused assets, with
        `docker_stub` first on PATH, and the stub's argv log."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        (root / "bin").mkdir()
        docker_log = root / "docker.log"
        docker = root / "bin" / "docker"
        docker.write_text(docker_stub.replace("@LOG@", str(docker_log)), encoding="utf-8")
        docker.chmod(0o755)
        deb, appimage = root / "a.deb", root / "a.AppImage"
        deb.write_bytes(b"deb")
        appimage.write_bytes(b"appimage")
        env = {k: v for k, v in os.environ.items() if k not in PROOF_KNOBS}
        env.update({
            "PATH": f"{root / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}",
            "REIFY_BUNDLE_DEB": str(deb),
            "REIFY_BUNDLE_APPIMAGE": str(appimage),
        })

        def run_proof(extra):
            return subprocess.run(
                [sys.executable, str(REPO_ROOT / PROOF)], env={**env, **extra},
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False,
            )

        return run_proof, docker_log

    def test_the_release_arm_env_arms_the_real_proof(self):
        """PROOF_ARM_ENV arms the real proof rather than an all-skipped green.

        Second link of the chain whose first is
        Release.test_the_proof_runs_once_armed_over_the_built_assets: unittest
        exits 0 when every test skips, so a renamed arm variable on either
        side would otherwise publish behind a vacuous proof. A failing stub
        docker stops the armed proof at `docker info`, before any build or
        container."""
        run_proof, docker_log = self._proof_runner(DOCKER_STUB)

        unarmed = run_proof({})
        self.assertIn("skipped", unarmed.stderr)
        self.assertFalse(docker_log.exists(), "the unarmed proof called docker")

        armed = run_proof(PROOF_ARM_ENV)
        self.assertNotEqual(armed.returncode, 0, armed.stderr)
        self.assertIn("info", docker_log.read_text(encoding="utf-8").split())

    def test_an_armed_proof_whose_docker_cannot_create_fails_before_any_container(self):
        run_proof, docker_log = self._proof_runner(DOCKER_CREATE_HANGS_STUB)
        armed = run_proof({**PROOF_ARM_ENV, "REIFY_DOCKER_CREATE_PROBE_SECS": "1"})
        self.assertNotEqual(armed.returncode, 0, armed.stderr)
        self.assertIn("docker create", armed.stderr)
        calls = docker_log.read_text(encoding="utf-8").splitlines()
        self.assertEqual([c for c in calls if c.startswith("run")], [], calls)


if __name__ == "__main__":
    unittest.main()
