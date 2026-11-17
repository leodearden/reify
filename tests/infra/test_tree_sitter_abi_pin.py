#!/usr/bin/env python3
"""
Both tree-sitter generators must emit a parser the workspace runtime can load.
The runtime is `tree-sitter = "0.24"`, which loads LANGUAGE_VERSION <= 14, while
tree-sitter-cli 0.25+ generates ABI 15 by default. On CLI 0.26.8 that default
applies only when the grammar dir has a tree-sitter.json, so the fixture grammar
carries one. A checkout that generated before the pin must heal on its next
build, not only a fresh clone. GitHub issue leodearden/reify#1, task 9727.
"""

import os
import re
import shlex
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

# The generator as it ran before the pin: every `--abi` argument is dropped,
# so the real CLI falls back to its own default ABI.
PRE_PIN_CLI_SHIM = """\
#!/usr/bin/env bash
args=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --abi) shift 2 || shift ;;
        --abi=*) shift ;;
        *) args+=("$1"); shift ;;
    esac
done
exec @REAL_CLI@ "${args[@]}"
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


def out_src_dir(target_dir: Path) -> Path:
    """The `src` dir build.rs generates into, beside the one archive under `target_dir`."""
    pattern = "debug/build/tree-sitter-reify-*/out/libtree_sitter_reify.a"
    archives = sorted(target_dir.glob(pattern))
    if len(archives) != 1:
        raise AssertionError(
            f"expected exactly one {target_dir}/{pattern}, found {len(archives)}"
        )
    return archives[0].parent / "src"


def env_without_abi_override(**extra: str) -> dict:
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
        cls.cli = shutil.which("tree-sitter")
        if cls.cli is None:
            raise unittest.SkipTest(
                "tree-sitter CLI not on PATH (install via: cargo install tree-sitter-cli)"
            )
        version = run([cls.cli, "--version"], REPO_ROOT, env_without_abi_override())
        cls.cli_version = version.stdout.strip()
        cls._require_cli_default_above_pinned_abi()

    @classmethod
    def _require_cli_default_above_pinned_abi(cls) -> None:
        """The control: without it, every pin assertion below passes vacuously."""
        with tempfile.TemporaryDirectory(prefix="ts-abi-pin-control-") as tmp:
            grammar_dir = Path(tmp)
            write_fixture_grammar(grammar_dir)
            result = run([cls.cli, "generate"], grammar_dir, env_without_abi_override())
            if result.returncode != 0:
                raise AssertionError(
                    f"{cls.cli_version}: a bare `tree-sitter generate` of the fixture "
                    f"grammar failed.\n{result.stdout}"
                )
            default_abi = language_version(grammar_dir / "src" / "parser.c")
        if default_abi <= PINNED_ABI:
            raise unittest.SkipTest(
                f"{cls.cli_version} generates ABI {default_abi} by default, not above the "
                f"pinned {PINNED_ABI}, so no test here can tell a pinned generator from an "
                f"unpinned one"
            )

    def _scratch_dir(self) -> Path:
        scratch = Path(tempfile.mkdtemp(prefix="ts-abi-pin-"))
        self.addCleanup(shutil.rmtree, scratch, True)
        return scratch

    def _pre_pin_env(self, **extra: str) -> dict:
        shim_dir = self._scratch_dir()
        shim = shim_dir / "tree-sitter"
        shim.write_text(PRE_PIN_CLI_SHIM.replace("@REAL_CLI@", shlex.quote(self.cli)))
        shim.chmod(0o755)
        return env_without_abi_override(
            PATH=f"{shim_dir}{os.pathsep}{os.environ.get('PATH', '')}", **extra
        )

    # ── scripts/tree-sitter-generate.sh ───────────────────────────────────────

    def _generate_script_fixture(self) -> Path:
        root = self._scratch_dir()
        shutil.copytree(
            REPO_ROOT / "scripts",
            root / "scripts",
            ignore=shutil.ignore_patterns("__pycache__"),
        )
        grammar_dir = root / "tree-sitter-reify"
        (grammar_dir / "src").mkdir(parents=True)
        write_fixture_grammar(grammar_dir)
        return root

    def _run_generate_script(self, root: Path, *args: str, env=None) -> subprocess.CompletedProcess:
        result = run(
            ["bash", str(root / "scripts" / "tree-sitter-generate.sh"), *args],
            root,
            env if env is not None else env_without_abi_override(),
        )
        self.assertEqual(result.returncode, 0, result.stdout)
        return result

    def test_generate_script_emits_pinned_abi(self):
        root = self._generate_script_fixture()
        result = self._run_generate_script(root, "--force")
        self.assertEqual(
            language_version(root / "tree-sitter-reify" / "src" / "parser.c"),
            PINNED_ABI,
            f"{self.cli_version}: scripts/tree-sitter-generate.sh emitted a parser "
            f"the tree-sitter 0.24 runtime cannot load.\n{result.stdout}",
        )

    def test_generate_script_regenerates_a_parser_generated_before_the_pin(self):
        root = self._generate_script_fixture()
        parser_c = root / "tree-sitter-reify" / "src" / "parser.c"

        self._run_generate_script(root, "--force", env=self._pre_pin_env())
        pre_pin_abi = language_version(parser_c)
        self.assertGreater(pre_pin_abi, PINNED_ABI, "the pre-pin shim did not take effect")

        healed = self._run_generate_script(root)
        self.assertEqual(
            language_version(parser_c),
            PINNED_ABI,
            f"{self.cli_version}: without --force, scripts/tree-sitter-generate.sh kept "
            f"the ABI-{pre_pin_abi} parser.c a pre-pin generate left behind its "
            f"consistent stamps.\n{healed.stdout}",
        )

        settled = self._run_generate_script(root)
        self.assertIn(
            "up to date",
            settled.stdout,
            "a parser generated at the pinned ABI must read as current, or every "
            "run pays a full tree-sitter generate",
        )

    # ── tree-sitter-reify/build.rs ────────────────────────────────────────────

    def _cargo_build(self, crate: Path, target_dir: Path, env: dict) -> subprocess.CompletedProcess:
        if shutil.which("cargo") is None:
            self.skipTest("cargo not on PATH")
        env = dict(env, CARGO_TARGET_DIR=str(target_dir))
        try:
            result = run(
                ["cargo", "build", "--offline", "--manifest-path", str(crate / "Cargo.toml")],
                crate,
                env,
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
        return result

    def test_build_rs_emits_pinned_abi(self):
        scratch = self._scratch_dir()
        crate = scratch / "crate"
        write_fixture_crate(crate)

        target_dir = scratch / "target"
        result = self._cargo_build(crate, target_dir, env_without_abi_override())
        self.assertEqual(
            language_version(out_src_dir(target_dir) / "parser.c"),
            PINNED_ABI,
            f"{self.cli_version}: tree-sitter-reify/build.rs emitted a parser "
            f"the tree-sitter 0.24 runtime cannot load.\n{result.stdout}",
        )

    def test_build_rs_regenerates_a_parser_generated_before_the_pin(self):
        scratch = self._scratch_dir()
        crate = scratch / "crate"
        write_fixture_crate(crate)
        target_dir = scratch / "target"

        self._cargo_build(crate, target_dir, self._pre_pin_env())
        pre_pin_abi = language_version(out_src_dir(target_dir) / "parser.c")
        self.assertGreater(pre_pin_abi, PINNED_ABI, "the pre-pin shim did not take effect")

        # The shim forwards `--version`, so the key stamp matches and only the
        # parser's own ABI line can trigger the heal.
        (crate / "build.rs").touch()
        healed = self._cargo_build(crate, target_dir, env_without_abi_override())
        self.assertEqual(
            language_version(out_src_dir(target_dir) / "parser.c"),
            PINNED_ABI,
            f"{self.cli_version}: tree-sitter-reify/build.rs kept the ABI-{pre_pin_abi} "
            f"parser.c a pre-pin build left behind its consistent stamps.\n{healed.stdout}",
        )


if __name__ == "__main__":
    unittest.main()
