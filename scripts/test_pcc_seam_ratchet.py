#!/usr/bin/env python3
"""
test_pcc_seam_ratchet.py — the INV-5 adoption ratchet for ε's public seams
(docs/prds/v0_6/prd-probe-gate-typed-outcomes.md D9, §5.5, §8 ε).  No test of
scripts/prd-capability-check.py (α) or scripts/prd-decompose-verify.py (pdv)
reads an α private name or patches α: they hand α a ProbeContext and a runner
instead.

Every count is over the parsed AST, never the raw text, so a comment or a
docstring that names a private counts nothing, and a patch call broken across
lines counts once.  Reads the test files as source; imports neither checker.
Runner: tests/infra/test_prd_capability_check.sh.
"""

import ast
import glob
import os
import unittest

from _pcc_test_support import _SCRIPTS_DIR

# The alias every α/pdv test binds the checker module to.
_ALPHA_NAME = "pcc"

# The name α is registered under in sys.modules, as a dotted patch target sees it.
_ALPHA_MODULE_PREFIX = "prd_capability_check."

_PATCH_HELPERS = frozenset({"object", "dict", "multiple"})

_SCOPE_GLOBS = (
    "test_pcc_*.py",
    "_pcc_test_support.py",
    "test_prd_decompose_verify*.py",
    "_pdv_test_support.py",
)


def _scoped_files():
    found = set()
    for pattern in _SCOPE_GLOBS:
        found.update(glob.glob(os.path.join(_SCRIPTS_DIR, pattern)))
    return sorted(found)


def _is_alpha(node):
    """True for a reference to α: the bare alias, or an attribute ending in it (pdv.pcc)."""
    if isinstance(node, ast.Name):
        return node.id == _ALPHA_NAME
    return isinstance(node, ast.Attribute) and node.attr == _ALPHA_NAME


def _is_private(name):
    return name.startswith("_") and not (name.startswith("__") and name.endswith("__"))


def _dotted(func):
    """A call target as a name tuple: unittest.mock.patch.object → (…, 'patch', 'object')."""
    parts = []
    while isinstance(func, ast.Attribute):
        parts.append(func.attr)
        func = func.value
    if isinstance(func, ast.Name):
        parts.append(func.id)
    return tuple(reversed(parts))


def _first_line(source, node):
    return ast.get_source_segment(source, node).splitlines()[0]


def _private_reads(tree, source):
    """α.<_private> attribute reads, and getattr(α, "_private") calls."""
    for node in ast.walk(tree):
        if isinstance(node, ast.Attribute) and _is_private(node.attr) and _is_alpha(node.value):
            yield node.lineno, _first_line(source, node)
        elif (isinstance(node, ast.Call) and _dotted(node.func) == ("getattr",)
              and len(node.args) >= 2 and _is_alpha(node.args[0])
              and isinstance(node.args[1], ast.Constant)
              and isinstance(node.args[1].value, str) and _is_private(node.args[1].value)):
            yield node.lineno, _first_line(source, node)


def _alpha_patches(tree, source):
    """patch.object / patch.dict / patch.multiple calls whose target is α."""
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and node.args and _is_alpha(node.args[0]):
            name = _dotted(node.func)
            if len(name) >= 2 and name[-2] == "patch" and name[-1] in _PATCH_HELPERS:
                yield node.lineno, _first_line(source, node)


def _alpha_string_patches(tree, source):
    """patch("prd_capability_check.<name>") calls: α patched by dotted path."""
    for node in ast.walk(tree):
        if (isinstance(node, ast.Call) and node.args
                and _dotted(node.func)[-1:] == ("patch",)
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)
                and node.args[0].value.startswith(_ALPHA_MODULE_PREFIX)):
            yield node.lineno, _first_line(source, node)


_COUNTERS = {
    "private reads": _private_reads,
    "α patches": _alpha_patches,
    "α string patches": _alpha_string_patches,
}


def _count(counter, source, label):
    tree = ast.parse(source, filename=label)
    return [(label, line, text) for line, text in counter(tree, source)]


# One instance of each counted form, plus prose and stdlib patches that must count
# nothing.  Single-line literals with the alias substituted in, so this file holds
# no patch site of its own for a line-based grep of scripts/ to find.
_SAMPLE = "\n".join((
    '"""Module prose naming {alpha}._prose_private."""',
    "import os, subprocess, sys, tempfile, unittest.mock",
    "from unittest.mock import patch",
    "# a comment naming {alpha}._commented_private",
    "read = {alpha}._read_private",
    "nested = pdv.{alpha}._nested_private",
    "dunder = {alpha}.__name__",
    "public = {alpha}.ProbeContext",
    "fetched = getattr({alpha}, '_fetched_private')",
    "one = unittest.mock.patch.object({alpha}, 'run_probe')",
    "two = unittest.mock.patch.object(",
    "    {alpha}, 'evaluate')",
    "three = unittest.mock.patch.dict({alpha}, {'A': 1})",
    "four = patch.multiple({alpha}, run_probe=None)",
    "five = unittest.mock.patch('prd_capability_check.run_probe')",
    "six = patch('prd_capability_check._find_repo_root')",
    "env = unittest.mock.patch.dict(os.environ, {'A': 'b'})",
    "run = unittest.mock.patch.object(subprocess, 'run')",
    "tmp = unittest.mock.patch.object(tempfile, 'tempdir', '/x')",
    "out = unittest.mock.patch('sys.stdout')",
    "def helper():",
    '    """Docstring naming {alpha}._doc_private."""',
    "    return None",
)).replace("{alpha}", _ALPHA_NAME)


class TestRatchetCountsCode(unittest.TestCase):
    """Each counter finds exactly the code instances in the sample, and no prose."""

    def _texts(self, counter):
        return sorted(text.strip() for _, _, text in _count(counter, _SAMPLE, "<sample>"))

    def test_private_reads(self):
        self.assertEqual(self._texts(_private_reads), sorted([
            f"{_ALPHA_NAME}._read_private",
            f"pdv.{_ALPHA_NAME}._nested_private",
            f"getattr({_ALPHA_NAME}, '_fetched_private')",
        ]))

    def test_alpha_patches_whatever_their_line_breaks(self):
        found = _count(_alpha_patches, _SAMPLE, "<sample>")
        self.assertEqual(len(found), 4, found)
        two_line_start = _SAMPLE.splitlines().index("two = unittest.mock.patch.object(") + 1
        self.assertIn(two_line_start, [line for _, line, _ in found])

    def test_alpha_string_patches(self):
        found = _count(_alpha_string_patches, _SAMPLE, "<sample>")
        self.assertEqual(len(found), 2, found)


class TestNoAlphaReachThrough(unittest.TestCase):
    """No α/pdv test file reads an α private or patches α."""

    def test_the_scope_covers_both_suites(self):
        names = {os.path.basename(path) for path in _scoped_files()}
        self.assertIn("_pcc_test_support.py", names)
        self.assertIn("_pdv_test_support.py", names)
        self.assertTrue(any(n.startswith("test_pcc_") for n in names), names)
        self.assertTrue(any(n.startswith("test_prd_decompose_verify") for n in names), names)

    def test_no_counter_finds_anything(self):
        for label, counter in _COUNTERS.items():
            with self.subTest(counter=label):
                found = []
                for path in _scoped_files():
                    with open(path, encoding="utf-8") as fh:
                        found += _count(counter, fh.read(), os.path.basename(path))
                found.sort()
                self.assertEqual(
                    found, [],
                    f"{label}:\n" + "\n".join(f"  {f}:{line}: {text}" for f, line, text in found),
                )


if __name__ == "__main__":
    unittest.main()
