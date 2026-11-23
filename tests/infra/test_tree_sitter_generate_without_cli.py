#!/usr/bin/env python3
"""
scripts/tree-sitter-generate.sh answers an up-to-date src/ without a tree-sitter
CLI on PATH, needs one for any generation, and ignores TREE_SITTER_BIN. Task 10264.
"""

import os
import shlex
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

FIXTURE_GRAMMAR_JS = (
    "module.exports = grammar({name: 'fixture', rules: {source_file: $ => 'a'}});\n"
)

STUB_CLI = """\
#!/bin/sh
echo "$*" >> @MARKER@
[ "$1" = generate ] || exit 0
abi=
want_abi=false
for arg in "$@"; do
    if [ "$want_abi" = true ]; then abi=$arg; want_abi=false; fi
    [ "$arg" = --abi ] && want_abi=true
done
mkdir -p src/tree_sitter
printf '#define LANGUAGE_VERSION %s\\n' "$abi" > src/parser.c
echo '{}' > src/grammar.json
echo '[]' > src/node-types.json
for h in alloc array parser; do : > "src/tree_sitter/$h.h"; done
"""


def _fixture_root(scratch: Path) -> Path:
    root = scratch / "root"
    shutil.copytree(
        REPO_ROOT / "scripts",
        root / "scripts",
        ignore=shutil.ignore_patterns("__pycache__"),
    )
    grammar_dir = root / "tree-sitter-reify"
    (grammar_dir / "src").mkdir(parents=True)
    (grammar_dir / "grammar.js").write_text(FIXTURE_GRAMMAR_JS)
    return root


def _write_stub_cli(directory: Path, marker: Path) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    stub = directory / "tree-sitter"
    stub.write_text(STUB_CLI.replace("@MARKER@", shlex.quote(str(marker))))
    stub.chmod(0o755)
    return stub


def _masked_path(directory: Path) -> str:
    directory.mkdir(parents=True, exist_ok=True)
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        if not entry or not os.path.isdir(entry):
            continue
        for name in sorted(os.listdir(entry)):
            if name == "tree-sitter":
                continue
            source = os.path.join(entry, name)
            link = directory / name
            if os.path.isdir(source) or not os.access(source, os.X_OK):
                continue
            if os.path.lexists(link):
                continue
            os.symlink(source, link)
    return str(directory)


def _run(root: Path, env: dict, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["bash", str(root / "scripts" / "tree-sitter-generate.sh"), *args],
        cwd=root,
        env=env,
        capture_output=True,
        text=True,
    )


def _output(result: subprocess.CompletedProcess) -> str:
    return f"rc={result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"


class TreeSitterGenerateWithoutCliTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        masked_dir = Path(tempfile.mkdtemp(prefix="ts-generate-without-cli-path-"))
        cls.addClassCleanup(shutil.rmtree, masked_dir, True)
        cls.masked_path = _masked_path(masked_dir)
        if shutil.which("tree-sitter", path=cls.masked_path) is not None:
            raise AssertionError(
                f"the masked PATH dir {cls.masked_path} still resolves a tree-sitter"
            )

    def setUp(self) -> None:
        scratch = Path(tempfile.mkdtemp(prefix="ts-generate-without-cli-"))
        self.addCleanup(shutil.rmtree, scratch, True)
        self.root = _fixture_root(scratch)
        self.src = self.root / "tree-sitter-reify" / "src"
        self.marker = scratch / "stub-runs.log"
        stub_dir = scratch / "stub-bin"
        stub = _write_stub_cli(stub_dir, self.marker)

        stub_env = dict(os.environ, PATH=f"{stub_dir}{os.pathsep}{os.environ['PATH']}")
        generated = _run(self.root, stub_env)
        self.assertEqual(generated.returncode, 0, _output(generated))
        current = _run(self.root, stub_env)
        self.assertEqual(current.returncode, 0, _output(current))
        self.assertIn("up to date", current.stdout, _output(current))
        self.assertEqual(self._stub_runs(), 1)

        self.masked_env = dict(os.environ, PATH=self.masked_path, TREE_SITTER_BIN=str(stub))

    def _stub_runs(self) -> int:
        return len(self.marker.read_text().splitlines())

    def _assert_refused_naming_the_cli(self, result: subprocess.CompletedProcess) -> None:
        self.assertNotEqual(result.returncode, 0, _output(result))
        self.assertIn("tree-sitter CLI not found", result.stderr, _output(result))
        self.assertEqual(self._stub_runs(), 1, "TREE_SITTER_BIN's stub ran")

    def test_a_current_src_needs_no_cli_on_path(self) -> None:
        result = _run(self.root, self.masked_env)
        self.assertEqual(result.returncode, 0, _output(result))
        self.assertIn("up to date", result.stdout, _output(result))

    def test_a_stale_src_without_a_cli_on_path_is_refused_naming_the_cli(self) -> None:
        grammar_stamp = self.src / ".grammar_hash.stamp"
        stamp_before = grammar_stamp.read_text()
        with (self.root / "tree-sitter-reify" / "grammar.js").open("a") as grammar:
            grammar.write("// edited\n")

        result = _run(self.root, self.masked_env)

        self._assert_refused_naming_the_cli(result)
        self.assertEqual(grammar_stamp.read_text(), stamp_before)

    def test_force_without_a_cli_on_path_is_refused_naming_the_cli(self) -> None:
        result = _run(self.root, self.masked_env, "--force")
        self._assert_refused_naming_the_cli(result)


if __name__ == "__main__":
    unittest.main()
