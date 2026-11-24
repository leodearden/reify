"""
seed_warm_lane_harness.py — the hermetic staging shared by unittests that run
the REAL warm-lane seed scripts (scripts/seed-warm-lane.sh and
scripts/seed-warm-base-initial.sh) against a fixture store.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the members import it via the sys.path idiom.

GIT: isolated_git_env() points every child git at an empty global config, no
system config, a ceiling at the fixture's tempdir and a fixed identity.
BASE GEN: stage_base_gen() writes the provenance sidecars refresh-warm-base.sh
writes beside a base target, so the seed reads a promoted gen's shape.
PATH: install_cp_stub() and write_executable() are refresh_warm_base_harness's,
so the refresh and seed members share one reflink-free `cp`.
"""

from pathlib import Path

from refresh_warm_base_harness import install_cp_stub, write_executable

__all__ = ("install_cp_stub", "isolated_git_env", "stage_base_gen", "write_executable")

IDENTITY = {
    "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.test",
    "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.test",
}


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
