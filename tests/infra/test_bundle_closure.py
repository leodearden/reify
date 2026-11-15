#!/usr/bin/env python3
"""
test_bundle_closure.py — stdlib unittest for scripts/bundle_closure.py, the
derivation of the shared-library set the Reify installer bundles (task 9732,
docs/prds/alpha-spine.md §5.2).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered
member is the thin wrapper tests/infra/test_bundle_closure.sh, which invokes
this file.

Hermetic: no docker, no ldd and no build. The derivation's docker probe is a
callable, so the fixed point is driven here by a fake target. ldd fixtures are
trimmed from real output (`env -u LD_LIBRARY_PATH ldd target/release/reify` on
the dev host, 2026-11-15) unless labelled DERIVED.
"""

import importlib.util
import json
import shlex
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL_PATH = REPO_ROOT / "scripts" / "bundle_closure.py"


def _load_tool():
    spec = importlib.util.spec_from_file_location("bundle_closure", TOOL_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {TOOL_PATH}")
    module = importlib.util.module_from_spec(spec)
    # Register before exec: @dataclass resolves a field's type through
    # sys.modules[cls.__module__], which is None for an unregistered module.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


bc = _load_tool()


HOST_LDD = """\
\tlinux-vdso.so.1 (0x00007cdc35b03000)
\tlibgmsh.so.4.15 => /opt/reify-deps/lib/libgmsh.so.4.15 (0x00007cdc32600000)
\tlibslvs.so.1 => /usr/lib/x86_64-linux-gnu/libslvs.so.1 (0x00007cdc33b41000)
\tlibtbb.so.12 => /opt/reify-deps/tbb-pin/libtbb.so.12 (0x00007cdc35a99000)
\tlibstdc++.so.6 => /opt/reify-deps/lib/libstdc++.so.6 (0x00007cdc31a00000)
\tlibTKernel.so.7.8 => /usr/lib/x86_64-linux-gnu/libTKernel.so.7.8 (0x00007cdc32434000)
\t/lib64/ld-linux-x86-64.so.2 (0x00007cdc35b05000)
\tlibTKBool.so.7.8 => /lib/x86_64-linux-gnu/libTKBool.so.7.8 (0x00007cdc2b800000)
"""

# DERIVED from two measurements: glibc 2.39's ldd given several files prints a
# `<file>:` header per file and, for a library short of a symbol version, an
# unindented `<file>: <lib>: version ... not found` line on stdout (measured with
# a version-scripted libv.so.1); the missing lines are from stock ubuntu:24.04.
PROBE_LDD = """\
/probe/usr/bin/reify:
/probe/usr/bin/reify: /lib/x86_64-linux-gnu/libstdc++.so.6: version `GLIBCXX_3.4.34' not found (required by /probe/usr/bin/reify)
\tlinux-vdso.so.1 (0x00007ffd3f1e2000)
\tlibgmsh.so.4.15 => /probe/usr/lib/Reify/lib/libgmsh.so.4.15 (0x00007f0a10000000)
\tlibfoo.so.1 => not found
\tlibstdc++.so.6 => /lib/x86_64-linux-gnu/libstdc++.so.6 (0x00007f0a0fc00000)
\t/lib64/ld-linux-x86-64.so.2 (0x00007f0a12a00000)
/probe/usr/bin/reify-gui:
\tlinux-vdso.so.1 (0x00007ffd3f1e3000)
\tlibfoo.so.1 => not found
"""


class ParseLdd(unittest.TestCase):

    def test_resolved_lines_map_soname_to_path(self):
        report = bc.parse_ldd(HOST_LDD)
        self.assertEqual(
            report.resolved["libgmsh.so.4.15"], "/opt/reify-deps/lib/libgmsh.so.4.15")
        self.assertEqual(
            report.resolved["libTKBool.so.7.8"], "/lib/x86_64-linux-gnu/libTKBool.so.7.8")
        self.assertEqual(report.missing, frozenset())
        self.assertEqual(report.version_short, frozenset())

    def test_vdso_and_bare_loader_line_are_ignored(self):
        report = bc.parse_ldd(HOST_LDD)
        self.assertEqual(
            set(report.resolved),
            {"libgmsh.so.4.15", "libslvs.so.1", "libtbb.so.12", "libstdc++.so.6",
             "libTKernel.so.7.8", "libTKBool.so.7.8"})

    def test_not_found_lines_are_missing(self):
        report = bc.parse_ldd(PROBE_LDD)
        self.assertEqual(report.missing, frozenset({"libfoo.so.1"}))
        self.assertNotIn("libfoo.so.1", report.resolved)

    def test_version_shortfall_names_the_short_library(self):
        report = bc.parse_ldd(PROBE_LDD)
        self.assertEqual(report.version_short, frozenset({"libstdc++.so.6"}))

    def test_per_file_headers_are_ignored(self):
        report = bc.parse_ldd(PROBE_LDD)
        self.assertEqual(set(report.resolved), {"libgmsh.so.4.15", "libstdc++.so.6"})

    def test_an_unrecognised_line_is_an_error_naming_it(self):
        with self.assertRaises(ValueError) as caught:
            bc.parse_ldd("\tnot a dynamic executable\n")
        self.assertIn("not a dynamic executable", str(caught.exception))

    def test_report_is_immutable(self):
        report = bc.parse_ldd(HOST_LDD)
        with self.assertRaises(Exception):
            report.missing = frozenset({"x"})
        with self.assertRaises(TypeError):
            report.resolved["x"] = "/y"


def _report(missing=(), version_short=()):
    return bc.LddReport(
        resolved={}, missing=frozenset(missing), version_short=frozenset(version_short))


class FakeTarget:
    """A target lacking A, B and C: B is needed only by A, so ldd reports it
    only once A is staged; C is present but short of a symbol version."""

    def __init__(self):
        self.calls = []

    def __call__(self, staged):
        self.calls.append(staged)
        missing = set()
        if "libA.so.1" not in staged:
            missing.add("libA.so.1")
        elif "libB.so.2" not in staged:
            missing.add("libB.so.2")
        short = set() if "libC.so.3" in staged else {"libC.so.3"}
        return _report(missing, short)


HOST = {
    "libA.so.1": "/opt/x/libA.so.1",
    "libB.so.2": "/opt/x/libB.so.2",
    "libC.so.3": "/opt/x/libC.so.3",
    "libD.so.4": "/usr/lib/libD.so.4",
}


class DeriveClosure(unittest.TestCase):

    def test_fixed_point_stages_transitive_and_version_short_libraries(self):
        target = FakeTarget()
        closure = bc.derive_closure(HOST, target)
        self.assertEqual(closure, frozenset({"libA.so.1", "libB.so.2", "libC.so.3"}))

    def test_probe_runs_until_a_round_reports_nothing(self):
        target = FakeTarget()
        bc.derive_closure(HOST, target)
        self.assertEqual(
            target.calls,
            [frozenset(),
             frozenset({"libA.so.1", "libC.so.3"}),
             frozenset({"libA.so.1", "libB.so.2", "libC.so.3"})])

    def test_a_target_lacking_nothing_bundles_nothing(self):
        self.assertEqual(bc.derive_closure(HOST, lambda staged: _report()), frozenset())

    def test_a_missing_library_the_host_cannot_supply_is_an_error_naming_it(self):
        with self.assertRaises(bc.ClosureError) as caught:
            bc.derive_closure(HOST, lambda staged: _report(missing={"libZ.so.9"}))
        self.assertIn("libZ.so.9", str(caught.exception))

    def test_a_round_that_stages_nothing_new_is_an_error_naming_what_remains(self):
        target = lambda staged: _report(version_short={"libA.so.1"})  # noqa: E731
        with self.assertRaises(bc.ClosureError) as caught:
            bc.derive_closure(HOST, target)
        self.assertIn("libA.so.1", str(caught.exception))


class TauriOverlay(unittest.TestCase):

    def setUp(self):
        self.overlay = bc.tauri_overlay(
            stage=Path("/stage"),
            product_name="Reify",
            main_binary=("reify-gui", Path("/t/release/reify-gui")),
            extra_binaries={"reify": Path("/t/release/reify")},
        )

    def test_deb_ships_the_staged_binaries_and_library_dir(self):
        self.assertEqual(
            self.overlay["bundle"]["linux"]["deb"]["files"],
            {"/usr/bin/reify-gui": "/stage/usr/bin/reify-gui",
             "/usr/bin/reify": "/stage/usr/bin/reify",
             "/usr/lib/Reify/lib": "/stage/usr/lib/Reify/lib"})

    def test_appimage_ships_only_the_unpatched_extra_binary(self):
        self.assertEqual(
            self.overlay["bundle"]["linux"]["appimage"]["files"],
            {"/usr/bin/reify": "/t/release/reify"})

    def test_overlay_is_json(self):
        self.assertEqual(json.loads(json.dumps(self.overlay)), self.overlay)


def _run_lines(dockerfile):
    return [line.strip()[len("RUN "):] for line in dockerfile.splitlines()
            if line.strip().startswith("RUN ")]


def _installed_packages(dockerfile):
    for run in _run_lines(dockerfile):
        for command in run.split("&&"):
            words = shlex.split(command)
            if words[:2] == ["apt-get", "install"]:
                return words, {w for w in words[2:] if not w.startswith("-")}
    raise AssertionError(f"no apt-get install in:\n{dockerfile}")


DEPENDS = ["libwebkit2gtk-4.1-0", "libgtk-3-0"]


class ReferenceImage(unittest.TestCase):

    def test_dockerfile_starts_from_stock_noble(self):
        lines = [line.strip() for line in bc.reference_dockerfile(DEPENDS).splitlines()
                 if line.strip() and not line.strip().startswith("#")]
        self.assertEqual(lines[0], "FROM ubuntu:24.04")

    def test_dockerfile_installs_exactly_the_depends_plus_patchelf(self):
        words, packages = _installed_packages(bc.reference_dockerfile(DEPENDS))
        self.assertEqual(packages, {"libwebkit2gtk-4.1-0", "libgtk-3-0", "patchelf"})
        self.assertIn("--no-install-recommends", words)

    def test_tag_is_stable_for_the_same_dockerfile(self):
        text = bc.reference_dockerfile(DEPENDS)
        self.assertEqual(bc.reference_image_tag(text), bc.reference_image_tag(text))
        self.assertTrue(bc.reference_image_tag(text).startswith("reify-bundle-ref:"))

    def test_tag_changes_when_the_depends_change(self):
        self.assertNotEqual(
            bc.reference_image_tag(bc.reference_dockerfile(DEPENDS)),
            bc.reference_image_tag(bc.reference_dockerfile(DEPENDS + ["libfuse2t64"])))


if __name__ == "__main__":
    unittest.main()
