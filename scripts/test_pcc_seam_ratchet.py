#!/usr/bin/env python3
"""
test_pcc_seam_ratchet.py — the INV-5 adoption ratchet for ε's public seams
(docs/prds/v0_6/prd-probe-gate-typed-outcomes.md D9, §5.5, §8 ε).  No test of
scripts/prd-capability-check.py (α) or scripts/prd-decompose-verify.py (pdv)
reads an α private name or patches α: they hand α a ProbeContext and a runner
instead.  Nor does any bind α to a name other than its one alias, where the
other counters could not follow it.

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

# The one alias α may be bound to in scope, held there by the rebindings counter.
_ALPHA_NAME = "pcc"

# The _pdv_test_support function that returns α to a pdv partition.
_ALPHA_LOADER = "load_pcc"

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
    """True for an expression that is α: the bare alias, an attribute ending in it
    (pdv.pcc), a load_pcc() call, or a conditional with α on a branch."""
    if isinstance(node, ast.Name):
        return node.id == _ALPHA_NAME
    if isinstance(node, ast.Attribute):
        return node.attr == _ALPHA_NAME
    if isinstance(node, ast.Call):
        return _dotted(node.func)[-1:] == (_ALPHA_LOADER,)
    if isinstance(node, ast.IfExp):
        return _is_alpha(node.body) or _is_alpha(node.orelse)
    if isinstance(node, ast.BoolOp):
        return any(_is_alpha(value) for value in node.values)
    return False


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


def _bound_name(target):
    """The name an assignment target binds; None for a subscript, tuple or starred target."""
    if isinstance(target, ast.Name):
        return target.id
    if isinstance(target, ast.Attribute):
        return target.attr
    return None


def _parameter_defaults(arguments):
    """(parameter, default) for each parameter that has a default."""
    positional = arguments.posonlyargs + arguments.args
    yield from zip(positional[len(positional) - len(arguments.defaults):], arguments.defaults)
    for parameter, default in zip(arguments.kwonlyargs, arguments.kw_defaults):
        if default is not None:
            yield parameter, default


def _name_bindings(tree):
    """(bound name, value, anchor) for each assignment, keyword argument and
    parameter default; the bound name is None where no name is bound."""
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign):
            for target in node.targets:
                yield _bound_name(target), node.value, node
        elif isinstance(node, (ast.AnnAssign, ast.NamedExpr)) and node.value is not None:
            yield _bound_name(node.target), node.value, node
        elif isinstance(node, ast.keyword):
            yield node.arg, node.value, node
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)):
            for parameter, default in _parameter_defaults(node.args):
                yield parameter.arg, default, node


def _aliased_alpha_imports(tree):
    """Imports of the alias or the loader under another name."""
    for node in ast.walk(tree):
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            for alias in node.names:
                if (alias.name in (_ALPHA_NAME, _ALPHA_LOADER)
                        and alias.asname not in (None, alias.name)):
                    yield node


def _alpha_rebindings(tree, source):
    """α bound to a name other than its alias, where no other counter can follow it."""
    for name, value, anchor in _name_bindings(tree):
        if name not in (None, _ALPHA_NAME) and _is_alpha(value):
            yield anchor.lineno, _first_line(source, anchor)
    for node in _aliased_alpha_imports(tree):
        yield node.lineno, _first_line(source, node)


_COUNTERS = {
    "private reads": _private_reads,
    "α patches": _alpha_patches,
    "α string patches": _alpha_string_patches,
    "α rebindings": _alpha_rebindings,
}


def _count(counter, source, label):
    tree = ast.parse(source, filename=label)
    return [(label, line, text) for line, text in counter(tree, source)]


# One instance of each counted form, plus prose, stdlib patches and non-α bindings
# (α bound to its own alias among them) that must count nothing.  Single-line
# literals with the alias substituted in, so this file holds no patch site of its
# own for a line-based grep of scripts/ to find.
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
    "loaded = load_pcc()._loaded_private",
    "checker = {alpha}",
    "self.checker = pdv.{alpha}",
    "chosen = load_pcc() if given is None else given",
    "fallback = given or load_pcc()",
    "typed: object = {alpha}",
    "print(named := load_pcc())",
    "require(test, checker={alpha})",
    "def pick(test, checker={alpha}):",
    "    return checker",
    "from _pcc_test_support import {alpha} as checker",
    "from _pdv_test_support import load_pcc as loader",
    "{alpha} = load_pcc() if {alpha} is None else {alpha}",
    "def fine(test, {alpha}=None):",
    "    return {alpha}",
    "schema = {alpha}.published_schemas()['result_record']",
    "done = subprocess.run(['true'])",
    "require(test, runner={alpha}.run_probe)",
    "sys.modules['prd_capability_check'] = {alpha}",
    "from _pcc_test_support import {alpha}",
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
            "load_pcc()._loaded_private",
        ]))

    def test_alpha_rebindings(self):
        self.assertEqual(self._texts(_alpha_rebindings), sorted([
            f"checker = {_ALPHA_NAME}",
            f"self.checker = pdv.{_ALPHA_NAME}",
            "chosen = load_pcc() if given is None else given",
            "fallback = given or load_pcc()",
            f"typed: object = {_ALPHA_NAME}",
            "named := load_pcc()",
            f"checker={_ALPHA_NAME}",
            f"def pick(test, checker={_ALPHA_NAME}):",
            f"from _pcc_test_support import {_ALPHA_NAME} as checker",
            "from _pdv_test_support import load_pcc as loader",
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
    """No α/pdv test file reads an α private, patches α, or binds α to another name."""

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
