#!/usr/bin/env python3
"""
test_refresh_warm_base_built_tree.py — stdlib unittest for the BUILT-TREE arm of
scripts/refresh-warm-base.sh's inv.9 provenance guard (task 9765).

A merge-role verify stamps target/reify-verify-attempt.json with the tree the
target was built FROM. The refresh must not label a gen with --landed-commit
unless that recorded tree is the landed commit's tree: a _merge-verify reset to
a landed commit without a rebuild otherwise promotes artifacts built from
another tree under the landed commit's name.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_refresh_warm_base_built_tree.sh.

HERMETIC: the refresh script runs from a temp scripts/ dir beside a no-op
prune-unconsulted-target.sh, `cp` is a PATH stub that drops --reflink=always,
and the lane is a temp git repository on main with an untracked target/.
"""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
REFRESH = REPO_ROOT / "scripts" / "refresh-warm-base.sh"
SIDECAR = "reify-verify-attempt.json"
ABSENT_OID = "0123456789abcdef0123456789abcdef01234567"


def write_executable(path, body):
    path.write_text(body)
    path.chmod(0o755)


def env_without(*prefixes):
    return {k: v for k, v in os.environ.items() if not k.startswith(prefixes)}


def sidecar_body(tree_oid):
    """verify.sh's attempt-0 stamp, byte for byte (its add_test_passes writer)."""
    return (f'{{"tree_oid":"{tree_oid}","profiles":"debug release",'
            f'"timestamp":"2026-11-01T06:20:01Z"}}\n')


class BuiltTreeHarness(unittest.TestCase):
    """Commit A tracks units.md ("v1") and unchanged.txt; commit_units() adds a
    commit changing only units.md. lane/target/ is the advancing dir."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-refresh-built-tree-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)
        self.stub_dir = self.tmp / "stub-bin"
        self.stub_dir.mkdir()
        write_executable(self.stub_dir / "cp", f"""#!/usr/bin/env bash
args=()
for a in "$@"; do
    [ "$a" = "--reflink=always" ] && continue
    args+=("$a")
done
exec {shutil.which("cp")} "${{args[@]}}"
""")
        scripts_dir = self.tmp / "scripts"
        scripts_dir.mkdir()
        self.refresh = scripts_dir / REFRESH.name
        shutil.copy2(REFRESH, self.refresh)
        write_executable(scripts_dir / "prune-unconsulted-target.sh", "#!/bin/sh\nexit 0\n")
        self.lane = self.tmp / "lane"
        self.lane.mkdir()
        self.git("init", "-q", "-b", "main")
        (self.lane / "unchanged.txt").write_text("unchanged\n")
        self.a = self.commit_units("v1")
        self.target = self.lane / "target"
        self.artifact = Path("debug") / "libplaceholder.rlib"
        (self.target / self.artifact).parent.mkdir(parents=True)
        (self.target / self.artifact).write_text("placeholder artifact\n")
        self.base = self.tmp / "base"

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.lane), *args], env=env_without("GIT_"),
                              check=True, capture_output=True, text=True).stdout.strip()

    def commit(self, *args):
        self.git("-c", "user.email=warm-lane-test@localhost", "-c", "user.name=Warm Lane Test",
                 "-c", "commit.gpgsign=false", "commit", "-q", "--no-verify", *args)
        return self.git("rev-parse", "HEAD")

    def commit_units(self, text):
        (self.lane / "units.md").write_text(f"{text}\n")
        self.git("add", "--", "units.md", "unchanged.txt")
        return self.commit("-m", f"fixture: units {text}")

    def commit_same_tree(self):
        return self.commit("--allow-empty", "-m", "fixture: same tree, new commit")

    def tree_of(self, rev):
        return self.git("rev-parse", f"{rev}^{{tree}}")

    def write_sidecar(self, tree_oid):
        self.write_sidecar_body(sidecar_body(tree_oid))

    def write_sidecar_body(self, body):
        (self.target / SIDECAR).write_text(body)

    def run_refresh(self, landed, base=None):
        run_env = env_without("GIT_")
        run_env["PATH"] = f"{self.stub_dir}{os.pathsep}{run_env.get('PATH', '')}"
        return subprocess.run(["bash", str(self.refresh), str(self.target),
                               str(base or self.base), "--landed-commit", landed],
                              capture_output=True, text=True, env=run_env, check=False)

    def gen_entries(self, base=None):
        name = (base or self.base).name
        return sorted(self.tmp.glob(f"{name}.gen.*"))

    def live_gen(self):
        self.assertTrue(self.base.is_symlink(), f"{self.base} is not a symlink")
        return Path(os.readlink(self.base))

    def basecommit_of(self, gen):
        return Path(f"{gen}.basecommit").read_text()


class Refusal(BuiltTreeHarness):
    def setUp(self):
        super().setUp()
        self.b = self.commit_units("v2")
        self.write_sidecar(self.tree_of(self.a))

    def test_a_target_built_from_another_tree_does_not_advance_the_base(self):
        result = self.run_refresh(self.b)

        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertFalse(os.path.lexists(self.base), "the base advanced over a foreign build")
        self.assertEqual(self.gen_entries(), [])
        self.assertIn(self.tree_of(self.a), result.stderr)
        self.assertIn(self.tree_of(self.b), result.stderr)

    def test_the_refusal_lists_the_paths_the_seed_would_miss(self):
        result = self.run_refresh(self.b)

        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertIn("units.md", result.stderr)
        self.assertNotIn("unchanged.txt", result.stderr)


class IncidentSequence(BuiltTreeHarness):
    def test_a_refusal_leaves_the_current_gen_and_its_stamp_untouched(self):
        self.write_sidecar(self.tree_of(self.a))
        first = self.run_refresh(self.a)
        self.assertEqual(first.returncode, 0, first.stderr)
        gen = self.live_gen()
        self.assertEqual(self.basecommit_of(gen), self.a)
        entries_before = self.gen_entries()

        b = self.commit_units("v2")
        second = self.run_refresh(b)

        self.assertNotEqual(second.returncode, 0, second.stderr)
        self.assertEqual(self.live_gen(), gen)
        self.assertEqual(self.basecommit_of(gen), self.a)
        self.assertTrue(gen.is_dir(), f"{gen} was removed")
        self.assertEqual(self.gen_entries(), entries_before)


class Promotion(BuiltTreeHarness):
    def test_a_target_built_from_the_landed_tree_advances_the_base(self):
        b = self.commit_units("v2")
        self.write_sidecar(self.tree_of(b))

        result = self.run_refresh(b)

        self.assertEqual(result.returncode, 0, result.stderr)
        gen = self.live_gen()
        self.assertEqual(self.basecommit_of(gen), b)
        self.assertTrue((gen / self.artifact).is_file(), f"{self.artifact} missing from {gen}")

    def test_the_check_compares_trees_not_commits(self):
        b = self.commit_units("v2")
        c = self.commit_same_tree()
        self.assertNotEqual(b, c)
        self.write_sidecar(self.tree_of(b))

        result = self.run_refresh(c)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.basecommit_of(self.live_gen()), c)


class UnusableClaim(BuiltTreeHarness):
    def test_a_sidecar_naming_no_usable_tree_is_refused(self):
        bodies = {
            "unknown": sidecar_body("unknown"),
            "no tree_oid key": '{"profiles":"debug"}\n',
            "empty file": "",
            "absent oid": sidecar_body(ABSENT_OID),
        }
        for index, (label, body) in enumerate(bodies.items()):
            with self.subTest(label):
                base = self.tmp / f"base-{index}"
                self.write_sidecar_body(body)

                result = self.run_refresh(self.a, base=base)

                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertFalse(os.path.lexists(base), f"{label}: the base advanced")
                self.assertEqual(self.gen_entries(base), [])


if __name__ == "__main__":
    unittest.main()
