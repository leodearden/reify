"""
seed_warm_lane_harness.py — the hermetic seed fixture shared by every unittest
that runs the REAL warm-lane seed scripts (scripts/seed-warm-lane.sh and
scripts/seed-warm-base-initial.sh) against a throwaway fixture store:
test_seed_depinfo_relocation.py (task 10167),
test_tree_sitter_stale_run_eviction.py SeededLaneEnsure (task 10168),
test_seed_warm_lane_canon_stamp.py and test_canon_launch_sites.py (task 10230).

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the members import it via the sys.path idiom.

POOL: a throwaway repo whose checkout plays _merge-verify (the build root),
one warm base gen stamped with the sidecars refresh-warm-base.sh writes, and
linked-worktree lanes of that repo seeded from a gen by the real seed.
ENV: hermetic_env() strips every git, cargo, rustc and warm-lane knob the
caller could leak in, and pins a private git identity, CARGO_HOME and the
toolchain rust-toolchain.toml names. STUB_CP stands in for the seed's reflink
clone, which a tmp filesystem cannot do.
GIT: isolated_git_env() points every child git at an empty global config, no
system config, a ceiling at the fixture's tempdir and a fixed identity.
BASE GEN: stage_base_gen() writes the provenance sidecars refresh-warm-base.sh
writes beside a base target, so the seed reads a promoted gen's shape.
PATH: install_cp_stub() is refresh_warm_base_harness's, so the refresh and
seed members share one reflink-free `cp`.
"""

import os
import stat
import subprocess
import tomllib
from pathlib import Path

from refresh_warm_base_harness import install_cp_stub

REPO_ROOT = Path(__file__).resolve().parents[2]
SEED = REPO_ROOT / "scripts" / "seed-warm-lane.sh"
TOOLCHAIN_PIN = REPO_ROOT / "rust-toolchain.toml"
SUBPROCESS_TIMEOUT_SECS = 600

STRIPPED_ENV_PREFIXES = ("GIT_", "CARGO_", "RUSTC_", "REIFY_WARM_LANE_", "REIFY_LANE_")
STRIPPED_ENV_KEYS = frozenset({"RUSTC", "RUSTFLAGS", "RUSTDOCFLAGS"})

IDENTITY = {
    "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.test",
    "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.test",
}

# The seed's reflink clone, as a physical copy: a tmp fs has no reflink.
STUB_CP = """#!/usr/bin/env bash
src=""
dest=""
for arg in "$@"; do
    case "$arg" in
        -*) ;;
        *) if [ -z "$src" ]; then src="$arg"; else dest="$arg"; fi ;;
    esac
done
exec /bin/cp -a "$src" "$dest"
"""


def pinned_toolchain():
    """The channel rust-toolchain.toml pins: the toolchain whose dep-info
    encoding production's _merge-verify records."""
    with TOOLCHAIN_PIN.open("rb") as handle:
        return tomllib.load(handle)["toolchain"]["channel"]


def write_file(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def write_executable(path, text):
    write_file(path, text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def run(argv, cwd, env):
    return subprocess.run([str(a) for a in argv], cwd=cwd, env=env, capture_output=True,
                          text=True, check=False, timeout=SUBPROCESS_TIMEOUT_SECS)


def isolated_git_env(tmp):
    """The GIT_* settings that keep a child git inside the fixture under `tmp`."""
    empty_global = Path(tmp, "gitconfig-global")
    empty_global.write_text("")
    return {
        "GIT_CONFIG_GLOBAL": str(empty_global),
        "GIT_CONFIG_NOSYSTEM": "1",
        # A plain dir in the tempdir must never discover an enclosing repo.
        "GIT_CEILING_DIRECTORIES": str(tmp),
        **IDENTITY,
    }


def stage_base_gen(target, *, base_commit, build_root):
    """Write `<target>.basecommit`, `<target>.buildroot` and the base dir's
    .warm-base-meta (empty RUSTFLAGS and INVOCATION) for the base `target`."""
    target = Path(target)
    Path(f"{target}.basecommit").write_text(f"{base_commit}\n")
    Path(f"{target}.buildroot").write_text(f"{build_root}\n")
    (target.parent / ".warm-base-meta").write_text("RUSTFLAGS=\nINVOCATION=\n")


def hermetic_env(tmp, stub_dir):
    """The caller's env minus every git, cargo, rustc and warm-lane knob it
    could leak in, plus a private git identity, CARGO_HOME and toolchain pin."""
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(STRIPPED_ENV_PREFIXES) and key not in STRIPPED_ENV_KEYS}
    empty_git_config = tmp / "empty-gitconfig"
    empty_git_config.touch()
    env.update(
        PATH=f"{stub_dir}{os.pathsep}{os.environ['PATH']}",
        GIT_CONFIG_GLOBAL=str(empty_git_config),
        GIT_CONFIG_NOSYSTEM="1",
        GIT_CEILING_DIRECTORIES=str(tmp),
        GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.test",
        GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.test",
        CARGO_HOME=str(tmp / "cargo-home"),
        CARGO_INCREMENTAL="0",
        RUSTC_WRAPPER="",
        RUSTFLAGS="",
        RUSTUP_TOOLCHAIN=pinned_toolchain(),
        REIFY_WARM_LANE_RERERE_ARM="0",
        REIFY_WARM_LANE_RESEED_TRASH_SYNC="1",
    )
    return env


class Pool:
    """A throwaway repo whose checkout plays _merge-verify, one warm base gen
    stamped as refresh-warm-base.sh stamps it, and linked-worktree lanes of
    the repo seeded from that gen."""

    def __init__(self, tmp, build_root_name):
        self.build_root = tmp / build_root_name
        self.gen = tmp / "base" / "target.gen.1"
        self.lanes = tmp / "lanes"
        stub_dir = tmp / "stubs"
        write_executable(stub_dir / "cp", STUB_CP)
        self.env = hermetic_env(tmp, stub_dir)
        self.base_commit = None

    def checked(self, argv, cwd, env=None):
        result = run(argv, cwd, env or self.env)
        if result.returncode != 0:
            raise AssertionError(f"{argv} exited {result.returncode}:\n{result.stderr}")
        return result

    def git(self, cwd, *args):
        return self.checked(["git", *args], cwd).stdout.strip()

    def write_build_root(self, files):
        for rel, text in files.items():
            write_file(self.build_root / rel, text)

    def commit_build_root(self):
        self.git(self.build_root, "init", "-q", "-b", "main")
        self.git(self.build_root, "add", "-A")
        self.git(self.build_root, "commit", "-qm", "fixture")
        self.base_commit = self.git(self.build_root, "rev-parse", "HEAD")

    def stamp_gen(self, target):
        """Clone `target` as the gen, with the stamps the seed reads."""
        self.gen.parent.mkdir(parents=True)
        self.checked(["/bin/cp", "-a", target, self.gen], self.gen.parent)
        Path(f"{self.gen}.buildroot").write_text(f"{self.build_root}\n")
        Path(f"{self.gen}.basecommit").write_text(f"{self.base_commit}\n")
        (self.gen.parent / ".warm-base-meta").write_text("RUSTFLAGS=\nINVOCATION=\n")

    def add_lane(self, name, branch=None):
        lane = self.lanes / name
        if len(str(lane)) == len(str(self.build_root)):
            raise ValueError(f"lane {lane} must differ in length from {self.build_root}")
        placement = ["-b", branch] if branch else ["--detach"]
        self.git(self.build_root, "worktree", "add", "-q", *placement, lane, self.base_commit)
        return lane

    def seed(self, lane, seed_script=SEED, gen=None):
        """Seed `lane` from `gen`, or from this pool's own gen when None."""
        source = self.gen if gen is None else gen
        return run(["bash", seed_script, source, lane, "--fresh-checkout"], lane, self.env)
