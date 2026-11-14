#!/usr/bin/env python3
"""
Both tree-sitter generators must emit a parser the workspace runtime can load.
The runtime is `tree-sitter = "0.24"`, which loads LANGUAGE_VERSION <= 14, while
tree-sitter-cli 0.25+ generates ABI 15 by default. On CLI 0.26.8 that default
applies only when the grammar dir has a tree-sitter.json, so the fixture grammar
carries one. GitHub issue leodearden/reify#1, task 9727.
"""

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
TS_DIR = REPO_ROOT / "tree-sitter-reify"

PINNED_ABI = 14

FIXTURE_GRAMMAR_JS = """\
module.exports = grammar({
  name: 'fixture',
  rules: {
    source_file: $ => repeat($.thing),
    thing: $ => 'a',
  }
});
"""

FIXTURE_TREE_SITTER_JSON = """\
{"grammars":[{"name":"fixture","scope":"source.fixture","path":".","file-types":["fixture"]}],"metadata":{"version":"0.0.0"}}
"""

LANGUAGE_VERSION_DEFINE = re.compile(r"^#define LANGUAGE_VERSION (\d+)$", re.MULTILINE)

FIXTURE_CARGO_TOML = """\
[package]
name = "tree-sitter-reify"
version = "0.0.0"
edition = "2021"
build = "build.rs"

[build-dependencies]
cc = "1"

[workspace]
"""

CARGO_BUILD_TIMEOUT_SECS = 600

OFFLINE_UNRESOLVED = re.compile(
    r"no matching package|failed to select a version|registry index was not found"
    r"|not found in registry|failed to download|attempting to make an HTTP request"
)


def language_version(parser_c: Path) -> int:
    if not parser_c.is_file():
        raise AssertionError(f"{parser_c} was not generated")
    match = LANGUAGE_VERSION_DEFINE.search(parser_c.read_text())
    if match is None:
        raise AssertionError(f"{parser_c} has no `#define LANGUAGE_VERSION <n>` line")
    return int(match.group(1))


def hermetic_env(**extra: str) -> dict:
    env = dict(os.environ)
    env.pop("TREE_SITTER_ABI_VERSION", None)
    env.update(extra)
    return env


def write_fixture_grammar(grammar_dir: Path) -> None:
    grammar_dir.mkdir(parents=True, exist_ok=True)
    (grammar_dir / "grammar.js").write_text(FIXTURE_GRAMMAR_JS)
    (grammar_dir / "tree-sitter.json").write_text(FIXTURE_TREE_SITTER_JSON)


def write_fixture_crate(crate: Path) -> None:
    (crate / "src").mkdir(parents=True)
    shutil.copy(TS_DIR / "build.rs", crate / "build.rs")
    shutil.copy(TS_DIR / "build_support.rs", crate / "build_support.rs")
    write_fixture_grammar(crate)
    (crate / "src" / "scanner.c").write_text("/* fixture: no external scanner */\n")
    (crate / "src" / "lib.rs").write_text("//! tree-sitter ABI-pin fixture crate.\n")
    (crate / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)


def run(argv: list, cwd: Path, env: dict, timeout=None) -> subprocess.CompletedProcess:
    return subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=timeout,
    )


class TreeSitterAbiPinTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if shutil.which("tree-sitter") is None:
            raise unittest.SkipTest(
                "tree-sitter CLI not on PATH (install via: cargo install tree-sitter-cli)"
            )
        version = run(["tree-sitter", "--version"], REPO_ROOT, hermetic_env())
        cls.cli_version = version.stdout.strip()

    def _scratch_dir(self) -> Path:
        scratch = Path(tempfile.mkdtemp(prefix="ts-abi-pin-"))
        self.addCleanup(shutil.rmtree, scratch, True)
        return scratch

    def _assert_fixture_reproduces_abi15_default(self) -> None:
        grammar_dir = self._scratch_dir()
        write_fixture_grammar(grammar_dir)
        result = run(["tree-sitter", "generate"], grammar_dir, hermetic_env())
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertGreater(
            language_version(grammar_dir / "src" / "parser.c"),
            PINNED_ABI,
            f"{self.cli_version}: a bare `tree-sitter generate` of the fixture grammar "
            f"no longer reproduces the post-0.25 ABI-15 default, so the pin "
            f"assertions in this suite cannot discriminate.\n{result.stdout}",
        )

    def test_fixture_reproduces_the_cli_default_abi_15(self):
        self._assert_fixture_reproduces_abi15_default()

    def test_generate_script_emits_pinned_abi(self):
        self._assert_fixture_reproduces_abi15_default()

        root = self._scratch_dir()
        shutil.copytree(
            REPO_ROOT / "scripts",
            root / "scripts",
            ignore=shutil.ignore_patterns("__pycache__"),
        )
        grammar_dir = root / "tree-sitter-reify"
        (grammar_dir / "src").mkdir(parents=True)
        write_fixture_grammar(grammar_dir)

        result = run(
            ["bash", str(root / "scripts" / "tree-sitter-generate.sh"), "--force"],
            root,
            hermetic_env(),
        )
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertEqual(
            language_version(grammar_dir / "src" / "parser.c"),
            PINNED_ABI,
            f"{self.cli_version}: scripts/tree-sitter-generate.sh emitted a parser "
            f"the tree-sitter 0.24 runtime cannot load.\n{result.stdout}",
        )

    def test_build_rs_emits_pinned_abi(self):
        self._assert_fixture_reproduces_abi15_default()
        if shutil.which("cargo") is None:
            self.skipTest("cargo not on PATH")

        scratch = self._scratch_dir()
        crate = scratch / "crate"
        write_fixture_crate(crate)

        try:
            result = run(
                ["cargo", "build", "--offline", "--manifest-path", str(crate / "Cargo.toml")],
                crate,
                hermetic_env(CARGO_TARGET_DIR=str(scratch / "target")),
                timeout=CARGO_BUILD_TIMEOUT_SECS,
            )
        except subprocess.TimeoutExpired:
            self.skipTest(
                f"cargo build timed out after {CARGO_BUILD_TIMEOUT_SECS} s "
                f"(cold/contended-cache environment)"
            )
        if result.returncode != 0 and OFFLINE_UNRESOLVED.search(result.stdout):
            self.skipTest(
                "cargo could not resolve dependencies offline (host registry cache lacks them)"
            )
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertEqual(
            language_version(crate / "src" / "parser.c"),
            PINNED_ABI,
            f"{self.cli_version}: tree-sitter-reify/build.rs emitted a parser "
            f"the tree-sitter 0.24 runtime cannot load.\n{result.stdout}",
        )


if __name__ == "__main__":
    unittest.main()
