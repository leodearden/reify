"""Hermetic Rust-source fixtures shared by the suites built on
tests/infra/rust_construct_ratchet.py: source-text builders and a per-test temp
dir standing in for the repo root.

Not a test module (no test_ prefix), so no runner collects it.
"""

import tempfile
import unittest
from pathlib import Path

import rust_construct_ratchet as rcr

TEST_MODULE = "#[cfg(test)]\nmod tests {\n    fn t() {\n%s    }\n}\n"


def rust_fn(*body_lines):
    return "fn fixture() {\n" + "".join(line + "\n" for line in body_lines) + "}\n"


def cfg_test_module(*body_lines):
    return TEST_MODULE % "".join(f"        {line}\n" for line in body_lines)


def write_file(root, rel, text, mode="w"):
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open(mode, encoding="utf-8") as handle:
        handle.write(text)
    return path


class TempRoot(unittest.TestCase):
    """A per-test temp dir standing in for the repo root."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, rel, text):
        return write_file(self.root, rel, text)

    def fresh_root(self):
        return Path(tempfile.mkdtemp(dir=self.root))

    def evaluate_one(self, row):
        (result,) = rcr.evaluate((row,), self.root)
        return result
