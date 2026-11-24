#!/usr/bin/env python3
"""
test_seed_warm_lane_canon_stamp.py — stdlib unittest for how
scripts/seed-warm-lane.sh --fresh-checkout treats a base target that carries
the canonical-build-path stamp (task 10230; dark-factory
plans/canonical-build-path-shared-cache-prd.md, boundary row 18 and C12).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_seed_warm_lane_canon_stamp.sh.

FIXTURE: every case builds a hermetic store in a private tempdir:
  - `repo/`        a `git init -b main` checkout with one fixture crate whose
                   integration test bakes a path through env!(), and, when the
                   case wants the primitive, a copy of scripts/canon_run.py;
  - `lanes/lane`   a linked worktree of repo/, the shape of a warm lane;
  - `base/target`  a cloned-from target whose build-script replay files bake
                   either CANON (a stamped base) or the base's real build root
                   (an unstamped one), with the .basecommit / .buildroot
                   sidecars refresh-warm-base.sh writes.
The REAL seed runs against it, with a `cp` stub that drops --reflink=always
so the clone is a real byte copy. No cargo and no namespace is needed.
"""

import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = Path(__file__).resolve().parents[2]
SEED = REPO_ROOT / "scripts" / "seed-warm-lane.sh"
STAMP_NAME = fx.load_canon_run().STAMP_NAME
JOURNAL_NAME = fx.load_canon_cache().JOURNAL_NAME
SOCKET_NAME = ".reify-sccache.4242.sock"
UNIT = "fixture-0123456789abcdef"
TAURI_UNIT = "tauri-0123456789abcdef"
IT_RS = Path("crates", "fixture", "tests", "it.rs")

FIXUP_LINE = re.compile(
    r"(Relocated|Relinked|Invalidated) [0-9]+"
    r"|Skipping (links-metadata|dep-info|env!\(\))"
    r"|relocate-depinfo-paths")

# Ambient state that would steer a child git, the hooks the seed arms, or the
# seed itself away from the fixture.
_AMBIENT_PREFIXES = ("GIT_", "REIFY_WARM_LANE_", "REIFY_STASH_GUARD_", "REIFY_MAIN_GATE_")


def _diag(completed):
    return (f"\n--- exit {completed.returncode} ---\n--- stdout ---\n"
            f"{completed.stdout}\n--- stderr ---\n{completed.stderr}")


def _write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def _tree_map(root):
    """{relpath: (file type, bytes when regular)} for every entry under root."""
    entries = {}
    for directory, dirnames, filenames in os.walk(root, followlinks=False):
        for name in dirnames + filenames:
            path = os.path.join(directory, name)
            mode = os.lstat(path).st_mode
            content = None
            if stat.S_ISREG(mode):
                with open(path, "rb") as handle:
                    content = handle.read()
            entries[os.path.relpath(path, root)] = (stat.S_IFMT(mode), content)
    return entries




class Fixture:
    """One store: repo, lane, and a base target baked for CANON or a real build root.

    stamped: the base target carries the stamp (stamp_content, by default
    CANON + "\\n"), a journal and a leftover socket, and its replay files bake
    CANON. Otherwise they bake the base's real build root, as today.
    primitive: the repo (so the lane) tracks scripts/canon_run.py.
    canon_via_symlink: CANON is spelled through a symlinked directory, so its
    realpath differs from the bytes the stamp and replay files hold.
    """

    def __init__(self, test, *, stamped, primitive, stamp_content=None,
                 canon_via_symlink=False):
        tmp = tempfile.TemporaryDirectory(prefix="test-seed-canon-stamp-")
        test.addCleanup(tmp.cleanup)
        self.test = test
        self.tmp = Path(tmp.name).resolve()
        if canon_via_symlink:
            (self.tmp / "canon-real").mkdir()
            os.symlink("canon-real", self.tmp / "canon-link")
            self.canon = str(self.tmp / "canon-link" / "ws")
        else:
            self.canon = str(self.tmp / "opt-canon" / "ws")
        merge_verify = self.tmp / "merge-verify"
        merge_verify.mkdir()
        self.merge_verify_rp = os.path.realpath(merge_verify)
        self.env = self._env()

        self.repo = self.tmp / "repo"
        self.repo.mkdir()
        self.git(self.repo, "init", "-q", "-b", "main")
        _write(self.repo / ".gitignore", "target/\n")
        _write(self.repo / "crates" / "fixture" / "src" / "lib.rs", "pub fn fixture() {}\n")
        _write(self.repo / IT_RS,
               "#[test]\nfn manifest_dir() {\n    let _ = env!(\"CARGO_MANIFEST_DIR\");\n}\n")
        if primitive:
            (self.repo / "scripts").mkdir()
            shutil.copy2(fx.CANON_RUN, self.repo / "scripts" / "canon_run.py")
        self.git(self.repo, "add", "-A")
        self.git(self.repo, "commit", "-q", "-m", "fixture base")
        base_commit = self.git(self.repo, "rev-parse", "HEAD").stdout.strip()

        self.lane = self.tmp / "lanes" / "lane"
        self.git(self.repo, "worktree", "add", "-q", "-b", "task/1", str(self.lane))
        self.lane_rp = os.path.realpath(self.lane)

        base = self.tmp / "base"
        self.base_target = base / "target"
        self._write_base_target(self.canon if stamped else self.merge_verify_rp)
        if stamped:
            _write(self.base_target / STAMP_NAME,
                   f"{self.canon}\n" if stamp_content is None else stamp_content)
            _write(self.base_target / JOURNAL_NAME, '{"event": "build"}\n')
            os.mknod(self.base_target / SOCKET_NAME, stat.S_IFSOCK | 0o600)
        _write(base / "target.basecommit", base_commit)
        _write(base / "target.buildroot", f"{merge_verify}\n")
        _write(base / ".warm-base-meta", "RUSTFLAGS=\nINVOCATION=\n")

    def _env(self):
        env = {k: v for k, v in os.environ.items() if not k.startswith(_AMBIENT_PREFIXES)}
        empty_global = self.tmp / "gitconfig-global"
        empty_global.write_text("")
        stub_bin = self.tmp / "stub-bin"
        stub_bin.mkdir()
        # No reflink on a tmp fs: drop the flag so the clone is a real byte copy.
        cp = stub_bin / "cp"
        cp.write_text(
            "#!/bin/sh\n"
            'for arg; do shift; [ "$arg" = "--reflink=always" ] || set -- "$@" "$arg"; done\n'
            f'exec {shutil.which("cp")} "$@"\n')
        cp.chmod(0o755)
        env.update({
            "PATH": f"{stub_bin}{os.pathsep}{os.environ.get('PATH', '')}",
            "GIT_CONFIG_GLOBAL": str(empty_global),
            "GIT_CONFIG_NOSYSTEM": "1",
            # A plain dir in the tempdir must never discover an enclosing repo.
            "GIT_CEILING_DIRECTORIES": str(self.tmp),
            "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.test",
            "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.test",
            "RUSTFLAGS": "",
            # Keeps the rerere guard's shared-config writes out of the fixture.
            "REIFY_WARM_LANE_RERERE_ARM": "0",
            "REIFY_WARM_LANE_RESEED_TRASH_SYNC": "1",
        })
        return env

    def _write_base_target(self, root):
        debug = self.base_target / "debug"
        _write(debug / ".fingerprint" / UNIT / "lib-fixture", "0123456789abcdef")
        out_dir = f"{root}/target/debug/build/{UNIT}/out"
        _write(debug / "build" / UNIT / "output",
               f"cargo:rerun-if-changed={root}/crates/fixture/build.rs\n"
               f"cargo:rustc-link-search=native={out_dir}\n")
        _write(debug / "build" / UNIT / "root-output", out_dir)
        _write(debug / "build" / TAURI_UNIT / "output",
               f"cargo:PERMISSION_FILES_PATH={root}/target/debug/build/{TAURI_UNIT}/out\n")

    def git(self, where, *args):
        completed = subprocess.run(["git", "-C", str(where), *args], env=self.env,
                                   capture_output=True, text=True)
        if completed.returncode != 0:
            self.test.fail(f"git {' '.join(args)} failed in {where}" + _diag(completed))
        return completed

    def run_seed(self):
        marker = self.tmp / "seed-start"
        marker.write_text("")
        # Read from the filesystem's own clock, which also stamps every touch the seed makes.
        self.seed_start_ns = marker.stat().st_mtime_ns
        return subprocess.run(
            ["bash", str(SEED), str(self.base_target), str(self.lane), "--fresh-checkout"],
            env=self.env, capture_output=True, text=True)

    @property
    def lane_target(self):
        return self.lane / "target"

    def it_rs_mtime_ns(self):
        return (self.lane / IT_RS).stat().st_mtime_ns

    def replay_files(self):
        build = self.lane_target / "debug" / "build" / UNIT
        return {name: (build / name).read_text() for name in ("output", "root-output")}


class SeedCase(unittest.TestCase):
    def assert_seeded(self, fixture, completed):
        self.assertEqual(completed.returncode, 0, _diag(completed))
        self.assertEqual(completed.stdout, f"{fixture.lane_target}\n", _diag(completed))

    def assert_adapted_from(self, fixture, completed, from_root):
        """Today's four fix-ups ran, taking from_root as the base's build root."""
        for name, content in fixture.replay_files().items():
            self.assertNotIn(from_root, content, f"{name} still names {from_root}" + _diag(completed))
            self.assertIn(fixture.lane_rp, content, f"{name} was not relocated" + _diag(completed))
        self.assertFalse((fixture.lane_target / "debug" / "build" / TAURI_UNIT).exists(),
                         "the tauri build dir was not invalidated" + _diag(completed))
        self.assertIn(f"relocate-depinfo-paths: rewrote 0 dep-info file(s) from {from_root} "
                      f"to {fixture.lane_rp};", completed.stderr)
        self.assertGreaterEqual(fixture.it_rs_mtime_ns(), fixture.seed_start_ns,
                                "it.rs was not relinked" + _diag(completed))
        self.assertIn("Relinked 1 ", completed.stderr)


class StampedWithPrimitive(SeedCase):
    """Row 18, first case: the lane builds at CANON, so its target stays the base's."""

    def test_the_lane_target_is_the_base_target_minus_runtime_files(self):
        fixture = Fixture(self, stamped=True, primitive=True)
        completed = fixture.run_seed()
        self.assert_seeded(fixture, completed)

        expected = _tree_map(fixture.base_target)
        del expected[JOURNAL_NAME]
        del expected[SOCKET_NAME]
        self.assertEqual(_tree_map(fixture.lane_target), expected, _diag(completed))
        fixups = [line for line in completed.stderr.splitlines() if FIXUP_LINE.search(line)]
        self.assertEqual(fixups, [], _diag(completed))
        self.assertLess(fixture.it_rs_mtime_ns(), fixture.seed_start_ns,
                        "a test source was touched" + _diag(completed))


class StampedWithoutPrimitive(SeedCase):
    """Row 18, second case: no primitive, so the lane builds at its real path."""

    def test_the_stamp_goes_and_the_target_is_adapted_from_canon(self):
        fixture = Fixture(self, stamped=True, primitive=False)
        completed = fixture.run_seed()
        self.assert_seeded(fixture, completed)

        for name in (STAMP_NAME, JOURNAL_NAME, SOCKET_NAME):
            self.assertFalse(os.path.lexists(fixture.lane_target / name),
                             f"{name} survived the seed" + _diag(completed))
        self.assert_adapted_from(fixture, completed, fixture.canon)

    def test_a_canon_under_a_symlinked_dir_is_rewritten_from_its_stamped_spelling(self):
        """The stamp's bytes are the literal prefix the clone's files bake, so they are matched unresolved."""
        fixture = Fixture(self, stamped=True, primitive=False, canon_via_symlink=True)
        self.assertNotEqual(os.path.realpath(fixture.canon), fixture.canon)
        completed = fixture.run_seed()
        self.assert_seeded(fixture, completed)

        for name in (STAMP_NAME, JOURNAL_NAME, SOCKET_NAME):
            self.assertFalse(os.path.lexists(fixture.lane_target / name),
                             f"{name} survived the seed" + _diag(completed))
        self.assert_adapted_from(fixture, completed, fixture.canon)


class Unstamped(SeedCase):
    """Row 18, third case: an unstamped base is adapted from its .buildroot, as today."""

    def test_the_target_is_adapted_from_the_recorded_build_root(self):
        fixture = Fixture(self, stamped=False, primitive=True)
        completed = fixture.run_seed()
        self.assert_seeded(fixture, completed)

        self.assertFalse(os.path.lexists(fixture.lane_target / STAMP_NAME), _diag(completed))
        self.assert_adapted_from(fixture, completed, fixture.merge_verify_rp)


class MalformedStamp(SeedCase):
    """A stamp that is no build root (absolute, no trailing slash, one line) fails the seed closed
    where it is read, before any fix-up runs, with or without the primitive."""

    def test_a_stamp_that_names_no_build_root_aborts_the_seed(self):
        for content in ("not/absolute\n", "\n", "/opt/x/\n", "/opt/x\n/opt/y\n"):
            for primitive in (True, False):
                with self.subTest(stamp=content, primitive=primitive):
                    fixture = Fixture(self, stamped=True, primitive=primitive,
                                      stamp_content=content)
                    completed = fixture.run_seed()
                    self.assertNotEqual(completed.returncode, 0, _diag(completed))
                    self.assertEqual(completed.stdout, "", _diag(completed))
                    self.assertIn(f"canonical-build-path stamp {fixture.lane_target / STAMP_NAME}",
                                  completed.stderr)
                    fixups = [line for line in completed.stderr.splitlines()
                              if FIXUP_LINE.search(line)]
                    self.assertEqual(fixups, [], _diag(completed))


if __name__ == "__main__":
    unittest.main()
