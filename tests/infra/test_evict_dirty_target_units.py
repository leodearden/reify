#!/usr/bin/env python3
"""
test_evict_dirty_target_units.py — stdlib unittest for
scripts/evict-dirty-target-units.py, the promote-time eviction of build-script
runs that the build root's cargo would rerun (task 10152).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_evict_dirty_target_units.sh.

FIXTURE TIMES: every fixture file and directory is stamped to a fixed past
time, and each case moves only the inputs it is about; no case reads the wall
clock. CargoHonoursEviction alone runs real cargo, on a two-crate workspace.
"""

import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "evict-dirty-target-units.py"

SECOND_NS = 10 ** 9
REF_NS = 1_700_000_000 * SECOND_NS
NEWER_NS = REF_NS + SECOND_NS
OLDER_NS = REF_NS - SECOND_NS
FIXTURE_NS = REF_NS - 3600 * SECOND_NS
SEED_STAMP_NS = int(datetime(2020, 1, 1, tzinfo=timezone.utc).timestamp()) * SECOND_NS

RUN_RECORD = "run-build-script-build-script-build"
CARGO_TIMEOUT_SECS = 600


def set_mtime_ns(path, ns):
    os.utime(path, ns=(ns, ns), follow_symlinks=False)


def stamp_tree(root, ns):
    """Stamp `root` and everything below it to `ns`, children before parents."""
    for dirpath, dirnames, filenames in os.walk(root, topdown=False):
        for name in filenames + dirnames:
            set_mtime_ns(os.path.join(dirpath, name), ns)
    set_mtime_ns(root, ns)


def write_file(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def make_workspace(root, members):
    """A cargo workspace of `members` under crates/, plus crates/shared/src/x.rs,
    which no member owns. Every file and directory is stamped to FIXTURE_NS."""
    listed = ", ".join(f'"crates/{m}"' for m in members)
    write_file(root / "Cargo.toml", f'[workspace]\nmembers = [{listed}]\nresolver = "2"\n')
    for member in members:
        crate = root / "crates" / member
        write_file(crate / "Cargo.toml",
                   f'[package]\nname = "{member}"\nversion = "0.1.0"\nedition = "2021"\n')
        write_file(crate / "build.rs", "fn main() {}\n")
        write_file(crate / "src" / "lib.rs", "")
    write_file(root / "crates" / "shared" / "src" / "x.rs", "")
    stamp_tree(root, FIXTURE_NS)


ENV_ONLY_LOCAL = [{"RerunIfEnvChanged": {"var": "X", "val": None}}]


def run_record_json(local):
    """The shape of cargo 1.96's run-build-script-*.json."""
    return {"rustc": 16696009966390864400, "features": "", "declared_features": "",
            "target": 0, "profile": 0, "path": 0,
            "deps": [[4674965233221187097, "build_script_build", False, 14320141023770759512]],
            "local": local, "rustflags": [], "config": 0, "compile_kind": 0}


def output_rel(profile_rel, pkg, hash16):
    return f"{profile_rel}/build/{pkg}-{hash16}/output"


def write_run_record(target, profile_rel, pkg, hash16, paths, extra_local=()):
    """A run unit whose `local` opens with RerunIfChanged over `paths`; see
    write_record. Returns the output path."""
    local = [{"RerunIfChanged": {"output": output_rel(profile_rel, pkg, hash16),
                                 "paths": list(paths)}},
             *ENV_ONLY_LOCAL, *extra_local]
    return write_record(target, profile_rel, pkg, hash16, local)


def write_record(target, profile_rel, pkg, hash16, local):
    """A run unit's fingerprint record (hash file + .json carrying `local`) and
    its output, stamped to REF_NS. Returns the output path."""
    unit_dir = target / profile_rel / ".fingerprint" / f"{pkg}-{hash16}"
    write_file(unit_dir / RUN_RECORD, "8a5654c15705d0b3")
    write_file(unit_dir / f"{RUN_RECORD}.json", json.dumps(run_record_json(local)))
    output = target / output_rel(profile_rel, pkg, hash16)
    write_file(output, "cargo:rerun-if-changed=build.rs\ncargo:rustc-env=BAKED=v1\n")
    set_mtime_ns(output, REF_NS)
    return output


def record_files(target, profile_rel, pkg, hash16):
    unit_dir = target / profile_rel / ".fingerprint" / f"{pkg}-{hash16}"
    return [unit_dir / RUN_RECORD, unit_dir / f"{RUN_RECORD}.json"]


def snapshot(paths):
    """Bytes and mtime of each path; None for a path that is gone."""
    return {str(p): (p.read_bytes(), p.stat().st_mtime_ns) if p.exists() else None
            for p in paths}


def env_without(*prefixes):
    return {k: v for k, v in os.environ.items() if not k.startswith(prefixes)}


class EvictHarness(unittest.TestCase):
    """tmp/ws is a workspace with member `alpha`; tmp/target is the target dir
    under judgement."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-evict-dirty-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name).resolve()
        self.root = self.tmp / "ws"
        make_workspace(self.root, ["alpha"])
        self.alpha = self.root / "crates" / "alpha"
        self.target = self.tmp / "target"
        self.target.mkdir()

    def run_evict(self, *args, env=None):
        argv = [str(SCRIPT), *map(str, args)]
        if not args:
            argv = [str(SCRIPT), "--build-root", str(self.root), str(self.target)]
        return subprocess.run(argv, capture_output=True, text=True, check=False,
                              env=env if env is not None else env_without("GIT_"))

    def assertEvicted(self, result, output, offending):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertFalse(output.exists(), f"{output} survived")
        rel = str(output.relative_to(self.target))
        self.assertRegex(result.stderr, re.compile(
            rf"^evicted {re.escape(rel)}: {re.escape(str(offending))} is ", re.M))


class Rule(EvictHarness):
    def test_a_run_with_a_newer_input_loses_its_output_and_keeps_its_record(self):
        file_out = write_run_record(self.target, "release", "alpha", "1111111111111111",
                                    ["build.rs"])
        dir_out = write_run_record(self.target, "debug", "alpha", "2222222222222222",
                                   ["../shared/src"])
        records = (record_files(self.target, "release", "alpha", "1111111111111111")
                   + record_files(self.target, "debug", "alpha", "2222222222222222"))
        before = snapshot(records)
        set_mtime_ns(self.alpha / "build.rs", NEWER_NS)
        set_mtime_ns(self.root / "crates" / "shared" / "src", NEWER_NS)

        result = self.run_evict()

        self.assertEvicted(result, file_out, self.alpha / "build.rs")
        self.assertEvicted(result, dir_out, self.root / "crates" / "shared" / "src")
        self.assertEqual(snapshot(records), before)

    def test_a_run_whose_inputs_are_older_or_tied_keeps_its_output(self):
        older = write_run_record(self.target, "release", "alpha", "3333333333333333",
                                 ["build.rs", "src/lib.rs"])
        tied = write_run_record(self.target, "debug", "alpha", "4444444444444444",
                                ["build.rs", "../shared/src"])
        set_mtime_ns(self.alpha / "src" / "lib.rs", OLDER_NS)
        set_mtime_ns(self.alpha / "build.rs", REF_NS)
        before = snapshot([older, tied])

        result = self.run_evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(snapshot([older, tied]), before)
        self.assertNotRegex(result.stderr, re.compile(r"^evicted ", re.M))

    def test_a_run_listing_a_missing_input_loses_its_output(self):
        output = write_run_record(self.target, "release", "alpha", "5555555555555555",
                                  ["build.rs", "src/generated.rs"])

        result = self.run_evict()

        self.assertEvicted(result, output, self.alpha / "src" / "generated.rs")


class Boundaries(EvictHarness):
    """What the evictor does not judge, and where it looks for records."""

    def tree(self):
        return snapshot(sorted(p for p in self.target.rglob("*") if p.is_file()))

    def test_an_input_outside_the_build_root_is_not_judged(self):
        outside = self.tmp / "outside.txt"
        write_file(outside, "the seed never re-stamps me\n")
        set_mtime_ns(outside, NEWER_NS)
        output = write_run_record(self.target, "release", "alpha", "6666666666666666",
                                  ["build.rs", str(outside)])
        before = snapshot([output])

        result = self.run_evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(snapshot([output]), before)

    def test_a_run_of_a_non_member_package_is_not_judged(self):
        output = write_run_record(self.target, "release", "anyhow", "7777777777777777",
                                  ["build.rs", "src/nowhere.rs"])
        before = snapshot([output])

        result = self.run_evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(snapshot([output]), before)

    def test_a_record_without_rerun_if_changed_is_not_judged(self):
        outputs = [
            write_record(self.target, "release", "alpha", "8888888888888888", ENV_ONLY_LOCAL),
            write_record(self.target, "debug", "alpha", "9999999999999999",
                         [{"Precalculated": "0.18.4"}]),
            write_record(self.target, "debug", "alpha", "aaaaaaaaaaaaaaaa",
                         [{"Precalculated": "0.1.0"}, *ENV_ONLY_LOCAL]),
        ]
        before = snapshot(outputs)

        result = self.run_evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(snapshot(outputs), before)

    def test_stale_runs_lose_their_outputs_under_every_profile_and_triple(self):
        profiles = ["debug", "release", "x86_64-unknown-linux-gnu/release"]
        outputs = [write_run_record(self.target, profile, "alpha", f"{i:016x}",
                                    ["src/generated.rs"])
                   for i, profile in enumerate(profiles, start=0xb0)]
        self.assertEqual(output_rel(profiles[2], "alpha", f"{0xb2:016x}"),
                         str(outputs[2].relative_to(self.target)))

        result = self.run_evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        for output in outputs:
            self.assertEvicted(result, output, self.alpha / "src" / "generated.rs")

    def test_a_stale_run_whose_output_is_already_absent_changes_nothing(self):
        output = write_run_record(self.target, "release", "alpha", "cccccccccccccccc",
                                  ["src/generated.rs"])
        output.unlink()
        before = self.tree()

        result = self.run_evict()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.tree(), before)

    def test_cargo_is_never_asked_without_a_rerun_if_changed_record(self):
        stub_dir = self.tmp / "stub-bin"
        calls = self.tmp / "cargo-calls"
        calls.touch()
        stub_dir.mkdir()
        stub = stub_dir / "cargo"
        stub.write_text(f'#!/bin/sh\necho "cargo $*" >> "{calls}"\nexit 1\n')
        stub.chmod(0o755)
        env = env_without("GIT_")
        env["PATH"] = f"{stub_dir}{os.pathsep}{env.get('PATH', '')}"

        with self.subTest("no run record at all"):
            result = self.run_evict(env=env)
            self.assertEqual(result.returncode, 0, result.stderr)
        with self.subTest("only records without RerunIfChanged"):
            write_record(self.target, "release", "alpha", "dddddddddddddddd", ENV_ONLY_LOCAL)
            result = self.run_evict(env=env)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls.read_text(), "")


class FailsClosed(EvictHarness):
    """Every case also holds a genuinely stale member run, which must keep its
    output: nothing is deleted until every record has been judged."""

    def setUp(self):
        super().setUp()
        self.stale = write_run_record(self.target, "release", "alpha", "eeeeeeeeeeeeeeee",
                                      ["src/generated.rs"])

    def write_raw_record(self, unit_dir_name, body):
        record = self.target / "debug" / ".fingerprint" / unit_dir_name / f"{RUN_RECORD}.json"
        write_file(record, body)
        return record

    def assertFailsClosed(self, result, names, reason=""):
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertNotIn("Traceback", result.stderr)
        self.assertTrue(self.stale.exists(), "an output was deleted before judging ended")
        self.assertRegex(result.stderr, re.compile(
            rf"^evict-dirty-target-units: {re.escape(str(names))}: .*{reason}", re.M))

    def test_a_record_that_is_not_json(self):
        record = self.write_raw_record("alpha-f000000000000001", "{not json")

        self.assertFailsClosed(self.run_evict(), record)

    def test_an_unknown_local_fingerprint_kind(self):
        local = [{"RerunIfChangedV2": {"output": "debug/build/alpha-f000000000000002/output",
                                       "paths": ["build.rs"]}}]
        record = self.write_raw_record("alpha-f000000000000002",
                                       json.dumps(run_record_json(local)))

        result = self.run_evict()

        self.assertFailsClosed(result, record, "RerunIfChangedV2")
        self.assertRegex(result.stderr, "cargo's fingerprint format changed")

    def test_a_malformed_rerun_if_changed_entry(self):
        out = "debug/build/alpha-f000000000000003/output"
        cases = {
            "no output": {"paths": ["build.rs"]},
            "non-list paths": {"output": out, "paths": "build.rs"},
            "non-string path": {"output": out, "paths": ["build.rs", 7]},
            "output outside the target": {"output": "../elsewhere/output", "paths": []},
            "absolute output": {"output": str(self.tmp / "output"), "paths": []},
        }
        for case, entry in cases.items():
            with self.subTest(case):
                record = self.write_raw_record(
                    "alpha-f000000000000003",
                    json.dumps(run_record_json([{"RerunIfChanged": entry}])))
                self.assertFailsClosed(self.run_evict(), record, "RerunIfChanged")

    def test_a_fingerprint_dir_outside_the_unit_grammar(self):
        for name in ["alpha-F000000000000004", "alpha-f00000000004", "f000000000000004"]:
            with self.subTest(name):
                record = self.write_raw_record(
                    name, json.dumps(run_record_json(ENV_ONLY_LOCAL)))
                self.assertFailsClosed(self.run_evict(), record, re.escape(name))
                shutil.rmtree(record.parent)

    def test_a_cargo_metadata_failure(self):
        (self.root / "Cargo.toml").unlink()

        result = self.run_evict()

        self.assertFailsClosed(result, self.root / "Cargo.toml", "cargo metadata")


class Usage(EvictHarness):
    def assertUsageError(self, result):
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertRegex(result.stderr, re.compile(r"^usage: ", re.M))

    def test_a_missing_build_root(self):
        self.assertUsageError(self.run_evict(self.target))

    def test_a_target_that_is_not_a_directory(self):
        not_dir = self.tmp / "target-file"
        not_dir.write_text("")
        for target in [not_dir, self.tmp / "absent"]:
            with self.subTest(str(target)):
                self.assertUsageError(self.run_evict("--build-root", self.root, target))

    def test_a_build_root_that_is_not_a_directory(self):
        write_run_record(self.target, "release", "alpha", "ffffffffffffffff", ["build.rs"])
        for root in [self.root / "Cargo.toml", self.tmp / "absent"]:
            with self.subTest(str(root)):
                self.assertUsageError(self.run_evict("--build-root", root, self.target))


BAKED_BUILD_RS = """\
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let inputs = manifest.join("../inputs");
    let value = inputs.join("value.txt");
    println!("cargo:rerun-if-changed={}", value.display());
    println!("cargo:rerun-if-changed={}", inputs.display());
    let baked = std::fs::read_to_string(&value).unwrap();
    println!("cargo:rustc-env=BAKED={}", baked.trim());
}
"""


def make_baked_workspace(ws):
    """`baked` bakes inputs/value.txt into BAKED via build.rs rustc-env; the
    `consumer` bin prints baked::BAKED."""
    write_file(ws / "Cargo.toml",
               '[workspace]\nmembers = ["baked", "consumer"]\nresolver = "2"\n')
    write_file(ws / "baked" / "Cargo.toml",
               '[package]\nname = "baked"\nversion = "0.1.0"\nedition = "2021"\n')
    write_file(ws / "baked" / "build.rs", BAKED_BUILD_RS)
    write_file(ws / "baked" / "src" / "lib.rs", 'pub const BAKED: &str = env!("BAKED");\n')
    write_file(ws / "consumer" / "Cargo.toml",
               '[package]\nname = "consumer"\nversion = "0.1.0"\nedition = "2021"\n\n'
               '[dependencies]\nbaked = { path = "../baked" }\n')
    write_file(ws / "consumer" / "src" / "main.rs",
               "fn main() {\n    println!(\"{}\", baked::BAKED);\n}\n")
    write_file(ws / "inputs" / "value.txt", "v1\n")


def baked_run_output(target):
    """The baked run's freshness reference, read from its RerunIfChanged record."""
    records = sorted(target.glob(f"debug/.fingerprint/baked-*/{RUN_RECORD}.json"))
    if len(records) != 1:
        raise AssertionError(f"expected one baked run record under {target}, found {records}")
    local = json.loads(records[0].read_text())["local"]
    outputs = [entry["RerunIfChanged"]["output"] for entry in local if "RerunIfChanged" in entry]
    if len(outputs) != 1:
        raise AssertionError(f"{records[0]} has no single RerunIfChanged entry: {local}")
    return target / outputs[0]


class CargoHonoursEviction(unittest.TestCase):
    """The incident in miniature: a lane whose sources the seed back-dated
    keeps a stale baked value from a promoted run reference, and loses it once
    the evictor has dropped that reference."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-evict-cargo-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name).resolve()
        self.ws = self.tmp / "ws"
        make_baked_workspace(self.ws)

    def cargo(self, target, *args):
        env = env_without("GIT_", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR")
        env.pop("RUSTC_WRAPPER", None)
        env.pop("CARGO_BUILD_RUSTC_WRAPPER", None)
        env["CARGO_INCREMENTAL"] = "0"
        env["CARGO_TARGET_DIR"] = str(target)
        result = subprocess.run(["cargo", *args, "--offline"], cwd=self.ws, env=env,
                                capture_output=True, text=True, check=False,
                                timeout=CARGO_TIMEOUT_SECS)
        self.assertEqual(result.returncode, 0, f"cargo {args}:\n{result.stderr}")
        return result

    def test_an_evicted_run_rebakes_while_an_unevicted_copy_stays_stale(self):
        built = self.tmp / "target_built"
        self.cargo(built, "build")
        output = baked_run_output(built)
        value = self.ws / "inputs" / "value.txt"
        value.write_text("v2\n")
        set_mtime_ns(value, output.stat().st_mtime_ns + SECOND_NS)
        staged = self.tmp / "staged"
        control = self.tmp / "control"
        shutil.copytree(built, staged, symlinks=True)
        shutil.copytree(built, control, symlinks=True)

        evict = subprocess.run([str(SCRIPT), "--build-root", str(self.ws), str(staged)],
                               capture_output=True, text=True, check=False)
        self.assertEqual(evict.returncode, 0, evict.stderr)
        stamp_tree(self.ws, SEED_STAMP_NS)

        self.assertEqual(self.cargo(control, "run", "-q", "-p", "consumer").stdout, "v1\n",
                         "the unevicted control did not reproduce the stale bake, "
                         "so this fixture cannot tell an eviction from a no-op")
        self.assertEqual(self.cargo(staged, "run", "-q", "-p", "consumer").stdout, "v2\n")
        settled = self.cargo(staged, "build", "-v")
        self.assertNotRegex(settled.stderr, re.compile(r"^\s*(Compiling|Dirty) baked ", re.M))


if __name__ == "__main__":
    unittest.main()
