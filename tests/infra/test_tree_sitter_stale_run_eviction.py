#!/usr/bin/env python3
"""
test_tree_sitter_stale_run_eviction.py — stdlib unittest for
`scripts/tree-sitter-freshness.sh evict-stale --build-root <worktree> <target>`,
the promote-time removal of tree-sitter-reify build-script runs whose input
stamp disagrees with the promoted tree (task 10168; PRD
warm-lane-pool-cow-seeding.md §9.5 inv.16).

EvictStale drives the mode as a CLI over synthetic cargo targets: run dirs
holding out/libtree_sitter_reify.a, their stamps and cargo's per-run markers,
plus each run's .fingerprint record. No case runs cargo. The stamp format is
never re-derived here: fingerprint_of() asks the script's own
--print-fingerprint.

RefreshStep runs a COPY of scripts/refresh-warm-base.sh, staged by
refresh_warm_base_harness beside the REAL freshness script, and pins its Step 3d.

SeededLaneEnsure is the task's acceptance case, end to end with no cargo:
_merge-verify's verify preflight (`ensure` in the build root), the refresh, the
REAL scripts/seed-warm-lane.sh into a linked-worktree lane, then `ensure` in
that lane. Its control arm stages the freshness sibling as a no-op stub and
must reproduce the forced rebuild, so the acceptance case cannot pass vacuously.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_tree_sitter_stale_run_eviction.sh.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import refresh_warm_base_harness as refresh_harness  # noqa: E402
from refresh_warm_base_harness import env_without  # noqa: E402
from seed_warm_lane_harness import Pool  # noqa: E402

REPO_ROOT = INFRA_DIR.parents[1]
FRESHNESS = REPO_ROOT / "scripts" / "tree-sitter-freshness.sh"
FRESHNESS_SIBLING = FRESHNESS.name
BASH = shutil.which("bash")

SECOND_NS = 10 ** 9
FIXTURE_NS = 1_600_000_000 * SECOND_NS
OLDER_NS = FIXTURE_NS + 100 * SECOND_NS
NEWER_NS = FIXTURE_NS + 200 * SECOND_NS
# seed-warm-lane.sh's bulk stamp, `touch -d 2020-01-01T00:00:00`: local time.
SEED_STAMP_NS = int(datetime(2020, 1, 1).timestamp()) * SECOND_NS

TS_PACKAGE = "tree-sitter-reify"
ARCHIVE = "libtree_sitter_reify.a"
STAMP = "tree_sitter_inputs.stamp"
RUN_RECORD = "run-build-script-build-script-build"
COMPILE_RECORD = "build-script-build-script-build"
WATCHED_INPUTS = ("grammar.js", "src/scanner.c")
HASHERS = ("sha256sum", "shasum")

# Bytes unlike the repo's own tree-sitter-reify, so the build root's
# fingerprint can never coincide with the script checkout's.
WS_INPUTS = {
    "grammar.js": "module.exports = grammar({ name: 'stale_run_eviction_fixture' });\n",
    "src/scanner.c": "/* stale-run-eviction fixture scanner */\n",
}

# The stamp build.rs wrote before #9896, as gen.656's dormant dirs carry it.
PRE_9896_STAMP = "".join(f"{digit * 64}  {rel}\n" for digit, rel in [
    ("1", "src/parser.c"),
    ("2", "src/scanner.c"),
    ("3", "src/tree_sitter/alloc.h"),
    ("4", "src/tree_sitter/array.h"),
    ("5", "src/tree_sitter/parser.h"),
])


def set_mtime_ns(path, ns):
    os.utime(path, ns=(ns, ns), follow_symlinks=False)


def write_file(path, text, mtime_ns=FIXTURE_NS):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    set_mtime_ns(path, mtime_ns)


def make_ws(root):
    """A build root holding tree-sitter-reify's two watched inputs."""
    for rel, text in WS_INPUTS.items():
        write_file(root / TS_PACKAGE / rel, text)
    return root


def build_root_files():
    """The tracked files of a build root that ignores its target/."""
    return {".gitignore": "/target\n",
            **{f"{TS_PACKAGE}/{rel}": text for rel, text in WS_INPUTS.items()}}


def bare_env():
    """The caller's env without any test-only freshness override or git knob."""
    return env_without("REIFY_TS_FRESHNESS_", "GIT_")


def run_freshness(*args, env=None):
    return subprocess.run([BASH, str(FRESHNESS), *map(str, args)], capture_output=True,
                          text=True, check=False, env=env if env is not None else bare_env())


def seam_env(ts_dir, target=None):
    """The script's documented test seam, pointed at a fixture."""
    env = {**bare_env(), "REIFY_TS_FRESHNESS_TS_DIR": str(ts_dir)}
    if target is not None:
        env["REIFY_TS_FRESHNESS_TARGET_DIR"] = str(target)
    return env


def fingerprint_of(ts_dir, env=None):
    """The stamp build.rs would write for `ts_dir`, from the script's own CLI."""
    result = run_freshness("--print-fingerprint", env={**(env or bare_env()),
                                                       "REIFY_TS_FRESHNESS_TS_DIR": str(ts_dir)})
    if result.returncode != 0:
        raise AssertionError(f"--print-fingerprint exited {result.returncode}:\n{result.stderr}")
    return result.stdout


def write_ts_run_dir(target, profile_rel, hash16, stamp, marker_ns):
    """A tree-sitter-reify build-script RUN unit: its run dir (archive, stamp
    unless None, cargo's per-run markers at `marker_ns`) and its .fingerprint
    record. Returns (run_dir, record_dir)."""
    name = f"{TS_PACKAGE}-{hash16}"
    run_dir = target / profile_rel / "build" / name
    write_file(run_dir / "out" / ARCHIVE, "!<arch>\nfixture archive\n")
    if stamp is not None:
        write_file(run_dir / "out" / STAMP, stamp)
    write_file(run_dir / "output", "cargo:rerun-if-changed=grammar.js\n", marker_ns)
    write_file(run_dir / "invoked.timestamp", "", marker_ns)
    record_dir = target / profile_rel / ".fingerprint" / name
    write_file(record_dir / RUN_RECORD, "8a5654c15705d0b3")
    write_file(record_dir / f"{RUN_RECORD}.json", json.dumps({"local": [], "deps": []}))
    return run_dir, record_dir


def write_ts_compile_dir(target, profile_rel, hash16):
    """The build-script COMPILE unit: a build-script-build binary, no out/.
    Returns (build_dir, record_dir)."""
    name = f"{TS_PACKAGE}-{hash16}"
    build_dir = target / profile_rel / "build" / name
    write_file(build_dir / "build-script-build", "\x7fELF fixture build script\n")
    record_dir = target / profile_rel / ".fingerprint" / name
    write_file(record_dir / COMPILE_RECORD, "0d2e5b1c0f6a4e77")
    write_file(record_dir / f"{COMPILE_RECORD}.json", json.dumps({"local": [], "deps": []}))
    return build_dir, record_dir


def write_other_run_dir(target, profile_rel, pkg, hash16):
    """Another package's build-script RUN unit with an out/ and a record."""
    name = f"{pkg}-{hash16}"
    run_dir = target / profile_rel / "build" / name
    write_file(run_dir / "out" / f"lib{pkg}_core.a", "!<arch>\nother archive\n")
    write_file(run_dir / "output", "cargo:rustc-link-lib=static=core\n")
    record_dir = target / profile_rel / ".fingerprint" / name
    write_file(record_dir / RUN_RECORD, "77aa11bb22cc33dd")
    write_file(record_dir / f"{RUN_RECORD}.json", json.dumps({"local": [], "deps": []}))
    return run_dir, record_dir


def tree_snapshot(root):
    """{path relative to root: (bytes, mtime_ns)} for every file under root."""
    snap = {}
    for dirpath, _dirnames, filenames in os.walk(root):
        for name in filenames:
            path = Path(dirpath) / name
            snap[str(path.relative_to(root))] = (path.read_bytes(),
                                                 path.lstat().st_mtime_ns)
    return snap


def without(snap, *dirs_rel):
    """`snap` minus every file under each of `dirs_rel`."""
    prefixes = tuple(f"{d}/" for d in dirs_rel)
    return {rel: v for rel, v in snap.items() if not rel.startswith(prefixes)}


def evicted_line(test, stdout, rel):
    """The one stdout line naming the eviction of `rel`."""
    lines = [line for line in stdout.splitlines() if f"evicted {rel} " in f"{line} "]
    test.assertEqual(len(lines), 1, f"expected one eviction line for {rel}:\n{stdout}")
    return lines[0]


def masked_path_dir(shim, masked):
    """Fill `shim` with symlinks to every executable on PATH except `masked`
    (first occurrence of a name wins), so PATH=<shim> is this host minus
    exactly those binaries. Masking by omission: a failing stub would drive
    the hasher's retry path, not its absence."""
    shim.mkdir()
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        if not entry or not os.path.isdir(entry):
            continue
        for name in sorted(os.listdir(entry)):
            source = os.path.join(entry, name)
            link = shim / name
            if name in masked or os.path.lexists(link):
                continue
            if os.path.isfile(source) and os.access(source, os.X_OK):
                link.symlink_to(source)
    return shim


class EvictStale(unittest.TestCase):
    """tmp/ws is the build root; tmp/target is the target under judgement."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-ts-evict-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name).resolve()
        self.ws = make_ws(self.tmp / "ws")
        self.ts_dir = self.ws / TS_PACKAGE
        self.fp = fingerprint_of(self.ts_dir)
        self.target = self.tmp / "target"
        self.target.mkdir()

    def evict(self, *args, env=None):
        if not args:
            args = ("--build-root", self.ws, self.target)
        return run_freshness("evict-stale", *args, env=env)

    def rel(self, path):
        return str(path.relative_to(self.target))

    def mixed_target(self):
        """Under both target depths: a current, a pre-#9896, an unstamped and
        an UNAVAILABLE run dir, a compile-unit dir and another package's run
        dir. Returns {kind: [(dir, record), ...]} with one entry per depth."""
        kinds = {kind: [] for kind in ("current", "pre_9896", "no_stamp", "unavailable",
                                       "compile", "other")}
        for profile_rel in ("debug", "x86_64-unknown-linux-gnu/release"):
            kinds["current"].append(write_ts_run_dir(
                self.target, profile_rel, "1111111111111111", self.fp, NEWER_NS))
            kinds["pre_9896"].append(write_ts_run_dir(
                self.target, profile_rel, "2222222222222222", PRE_9896_STAMP, OLDER_NS))
            kinds["no_stamp"].append(write_ts_run_dir(
                self.target, profile_rel, "3333333333333333", None, OLDER_NS))
            kinds["unavailable"].append(write_ts_run_dir(
                self.target, profile_rel, "4444444444444444", "UNAVAILABLE\n", OLDER_NS))
            kinds["compile"].append(write_ts_compile_dir(
                self.target, profile_rel, "5555555555555555"))
            kinds["other"].append(write_other_run_dir(
                self.target, profile_rel, "ring", "6666666666666666"))
        return kinds

    def test_non_matching_run_dirs_go_with_their_records_and_nothing_else_moves(self):
        kinds = self.mixed_target()
        stale_kinds = ("pre_9896", "no_stamp", "unavailable")
        evicted = [path for kind in stale_kinds for pair in kinds[kind] for path in pair]
        before = tree_snapshot(self.target)

        result = self.evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        for path in evicted:
            self.assertFalse(os.path.lexists(path), f"{path} survived")
        self.assertEqual(tree_snapshot(self.target),
                         without(before, *(self.rel(path) for path in evicted)))

    def test_each_eviction_is_named_on_stdout_with_its_reason(self):
        kinds = self.mixed_target()
        reasons = {
            "pre_9896": "first differing input: grammar.js",
            "no_stamp": f"no {STAMP}",
            "unavailable": "stamp is UNAVAILABLE",
        }

        result = self.evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        for kind, reason in reasons.items():
            for run_dir, _record in kinds[kind]:
                with self.subTest(kind=kind, run_dir=self.rel(run_dir)):
                    self.assertIn(reason, evicted_line(self, result.stdout, self.rel(run_dir)))
        for kept in ("current", "compile", "other"):
            for kept_dir, _record in kinds[kept]:
                self.assertNotIn(self.rel(kept_dir), result.stdout)

    def test_the_fingerprint_is_the_build_roots_not_the_script_checkouts(self):
        repo_fp = fingerprint_of(REPO_ROOT / TS_PACKAGE)
        self.assertNotEqual(repo_fp, self.fp, "the fixture must differ from the repo's bytes")
        repo_stamped = write_ts_run_dir(self.target, "debug", "7777777777777777",
                                        repo_fp, OLDER_NS)
        ws_stamped = write_ts_run_dir(self.target, "debug", "8888888888888888",
                                      self.fp, NEWER_NS)
        before = tree_snapshot(self.target)

        result = self.evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(os.path.lexists(repo_stamped[0]), "a repo-stamped run dir survived")
        self.assertFalse(os.path.lexists(repo_stamped[1]), "its record survived")
        self.assertEqual(tree_snapshot(self.target),
                         without(before, *(self.rel(path) for path in repo_stamped)))
        self.assertTrue((ws_stamped[0] / "out" / ARCHIVE).is_file())

    def test_a_target_without_archives_needs_no_tree_sitter_sources(self):
        write_ts_compile_dir(self.target, "debug", "5555555555555555")
        write_other_run_dir(self.target, "debug", "ring", "6666666666666666")
        bare_root = self.tmp / "bare-root"
        bare_root.mkdir()
        before = tree_snapshot(self.target)

        result = self.evict("--build-root", bare_root, self.target)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(tree_snapshot(self.target), before)

    def test_a_missing_input_fails_closed_naming_it(self):
        write_ts_run_dir(self.target, "debug", "2222222222222222", PRE_9896_STAMP, OLDER_NS)
        (self.ts_dir / "src" / "scanner.c").unlink()
        before = tree_snapshot(self.target)

        result = self.evict()

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("scanner.c", result.stderr)
        self.assertEqual(tree_snapshot(self.target), before)

    def test_a_host_without_a_hasher_skips_the_whole_run(self):
        self.mixed_target()
        shim = masked_path_dir(self.tmp / "no-hasher-bin", HASHERS)
        env = {**bare_env(), "PATH": str(shim)}
        probe = run_freshness("--print-fingerprint", env={**env, "REIFY_TS_FRESHNESS_TS_DIR":
                                                          str(self.ts_dir)})
        self.assertEqual((probe.returncode, probe.stdout), (0, "UNAVAILABLE\n"),
                         "the masked PATH still carries a hasher: the case would be vacuous")
        before = tree_snapshot(self.target)

        result = self.evict(env=env)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("SKIP", result.stdout)
        self.assertEqual(tree_snapshot(self.target), before)

    def test_usage_errors_exit_2_and_delete_nothing(self):
        write_ts_run_dir(self.target, "debug", "2222222222222222", PRE_9896_STAMP, OLDER_NS)
        not_dir = self.tmp / "a-file"
        not_dir.write_text("")
        absent = self.tmp / "absent"
        cases = {
            "no --build-root": (self.target,),
            "--build-root without a value": (self.target, "--build-root"),
            "--build-root is a file": ("--build-root", not_dir, self.target),
            "--build-root is absent": ("--build-root", absent, self.target),
            "no TARGET_DIR": ("--build-root", self.ws),
            "TARGET_DIR is absent": ("--build-root", self.ws, absent),
            "TARGET_DIR is a file": ("--build-root", self.ws, not_dir),
            "an extra positional": ("--build-root", self.ws, self.target, self.target),
        }
        before = tree_snapshot(self.target)
        for why, args in cases.items():
            with self.subTest(why):
                result = self.evict(*args)

                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertNotIn("unknown mode", result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertEqual(tree_snapshot(self.target), before)

    def test_ensure_after_an_eviction_is_fresh_and_touches_nothing(self):
        self.mixed_target()
        evicted = self.evict()
        self.assertEqual(evicted.returncode, 0, evicted.stderr)
        inputs = [self.ts_dir / rel for rel in WATCHED_INPUTS]
        before = [path.stat().st_mtime_ns for path in inputs]

        ensured = run_freshness("ensure", env=seam_env(self.ts_dir, self.target))

        self.assertEqual(ensured.returncode, 0, ensured.stderr)
        self.assertIn("FRESH", ensured.stdout)
        self.assertNotIn("touched", ensured.stdout)
        self.assertEqual([path.stat().st_mtime_ns for path in inputs], before)


class RefreshStep(unittest.TestCase):
    """refresh-warm-base.sh's Step 3d runs the freshness sibling's evict-stale
    on the staging copy, judged against the advancing worktree. The lane is a
    git repository on main whose untracked target/ advances."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-ts-evict-refresh-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name).resolve()
        self.stub_dir = self.tmp / "stub-bin"
        self.stub_dir.mkdir()
        refresh_harness.install_cp_stub(self.stub_dir)
        self.lane = self.tmp / "lane"
        for rel, text in build_root_files().items():
            write_file(self.lane / rel, text)
        refresh_harness.init_lane(self.lane)
        refresh_harness.git(self.lane, "add", "--", ".")
        self.head = refresh_harness.commit(self.lane, "-m", "fixture: advancing workspace")
        self.advancing = self.lane / "target"
        fp = fingerprint_of(self.lane / TS_PACKAGE)
        self.current = write_ts_run_dir(self.advancing, "debug", "1111111111111111", fp,
                                        NEWER_NS)
        self.stale = write_ts_run_dir(self.advancing, "debug", "2222222222222222",
                                      PRE_9896_STAMP, OLDER_NS)
        self.compile = write_ts_compile_dir(self.advancing, "debug", "5555555555555555")
        self.base = self.tmp / "base"

    def run_refresh(self, real=(FRESHNESS_SIBLING,), absent=()):
        refresh = refresh_harness.stage_refresh(self.tmp / "scripts", real=real, absent=absent)
        return refresh_harness.run_refresh(refresh, self.stub_dir, self.advancing, self.base,
                                           self.head)

    def in_gen(self, path):
        return Path(os.readlink(self.base)) / path.relative_to(self.advancing)

    def test_the_new_gen_loses_the_stale_run_and_keeps_the_rest(self):
        kept = [*self.current, *self.compile]
        before = {str(path): tree_snapshot(path) for path in kept}

        result = self.run_refresh()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        for path in self.stale:
            self.assertFalse(os.path.lexists(self.in_gen(path)), f"{path} was promoted")
        self.assertEqual({str(path): tree_snapshot(self.in_gen(path)) for path in kept}, before)
        self.assertIn(f"evicted {self.stale[0].relative_to(self.advancing)} ", result.stderr)

    def test_the_advancing_target_is_untouched(self):
        before = tree_snapshot(self.advancing)

        result = self.run_refresh()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(tree_snapshot(self.advancing), before)

    def test_a_missing_freshness_sibling_fails_the_refresh_closed(self):
        result = self.run_refresh(real=(), absent=(FRESHNESS_SIBLING,))

        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertFalse(os.path.lexists(self.base), "the base advanced without the eviction")
        self.assertEqual(sorted(self.tmp.glob("base.gen.*.partial")), [])


class SeededLaneEnsure(unittest.TestCase):
    """Promote, seed, then a lane's verify preflight. The build root's target
    carries a live current run dir and a dormant pre-#9896 one, as gen.656 did;
    `ensure` in the build root writes the ledger and its mtime witness first,
    as _merge-verify's own verify preflight does."""

    def setUp(self):
        tmp = Path(tempfile.mkdtemp(prefix="test-ts-evict-seed-")).resolve()
        self.addCleanup(shutil.rmtree, tmp, True)
        self.tmp = tmp
        self.pool = Pool(tmp, "build-root")
        self.pool.write_build_root(build_root_files())
        self.pool.commit_build_root()
        build_ts = self.pool.build_root / TS_PACKAGE
        self.advancing = self.pool.build_root / "target"
        fp = fingerprint_of(build_ts)
        write_ts_run_dir(self.advancing, "debug", "1111111111111111", fp, NEWER_NS)
        self.stale = write_ts_run_dir(self.advancing, "debug", "2222222222222222",
                                      PRE_9896_STAMP, OLDER_NS)
        preflight = run_freshness("ensure", env=seam_env(build_ts, self.advancing))
        self.assertEqual(preflight.returncode, 0, preflight.stderr)
        self.stub_dir = tmp / "refresh-stub-bin"
        self.stub_dir.mkdir()
        refresh_harness.install_cp_stub(self.stub_dir)
        self.base = tmp / "base" / "target"
        self.base.parent.mkdir()

    def promote_seed_and_ensure(self, real):
        """Refresh the base from the build root with the freshness sibling real
        (or a no-op stub), seed a fresh lane from the new gen, and run `ensure`
        in it. Returns (gen, lane's watched-input mtimes after the seed,
        ensure's result, the mtimes after ensure)."""
        refresh = refresh_harness.stage_refresh(self.tmp / "scripts", real=real)
        refreshed = refresh_harness.run_refresh(refresh, self.stub_dir, self.advancing,
                                                self.base, self.pool.base_commit)
        self.assertEqual(refreshed.returncode, 0, refreshed.stderr)
        gen = Path(os.readlink(self.base))
        lane = self.pool.add_lane("_lane-1")
        seeded = self.pool.seed(lane, gen=gen)
        self.assertEqual(seeded.returncode, 0, seeded.stderr)
        self.assertEqual(seeded.stdout, f"{lane / 'target'}\n", seeded.stderr)
        inputs = [lane / TS_PACKAGE / rel for rel in WATCHED_INPUTS]
        seeded_mtimes = [path.stat().st_mtime_ns for path in inputs]
        ensured = run_freshness("ensure", env=seam_env(lane / TS_PACKAGE, lane / "target"))
        return gen, seeded_mtimes, ensured, [path.stat().st_mtime_ns for path in inputs]

    def in_gen(self, gen, path):
        return gen / path.relative_to(self.advancing)

    def test_a_seeded_lanes_ensure_is_fresh_and_keeps_the_seed_stamp(self):
        gen, seeded, ensured, after = self.promote_seed_and_ensure(real=(FRESHNESS_SIBLING,))

        for path in self.stale:
            self.assertFalse(os.path.lexists(self.in_gen(gen, path)), f"{path} was promoted")
        self.assertEqual(ensured.returncode, 0, ensured.stderr)
        self.assertIn("FRESH", ensured.stdout)
        self.assertNotIn("touched", ensured.stdout)
        self.assertEqual(seeded, [SEED_STAMP_NS] * len(WATCHED_INPUTS))
        self.assertEqual(after, seeded)

    def test_control_without_the_eviction_the_lanes_ensure_forces(self):
        gen, seeded, ensured, after = self.promote_seed_and_ensure(real=())

        self.assertTrue(self.in_gen(gen, self.stale[0] / "out" / ARCHIVE).is_file(),
                        "the control's gen must carry the stale run dir")
        self.assertEqual(ensured.returncode, 0, ensured.stderr)
        for rel in WATCHED_INPUTS:
            self.assertIn(f"touched {rel}", ensured.stdout)
        self.assertEqual(seeded, [SEED_STAMP_NS] * len(WATCHED_INPUTS))
        for before, now in zip(seeded, after):
            self.assertGreater(now, before)


if __name__ == "__main__":
    unittest.main()
