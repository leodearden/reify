#!/usr/bin/env python3
"""
test_pcc_outcome_ratchet.py — the INV-5 adoption ratchet for the typed probe
outcome (docs/prds/v0_6/prd-probe-gate-typed-outcomes.md D1/D2, §8 γ).  How a
probe's machinery failed and whether its fixture existed are typed fields, so
scripts/prd-capability-check.py and scripts/prd-decompose-verify.py hold no
stderr sentinel, no ENOENT text route, and exactly one reader of tree-sitter's
load-failure signature.

Every count is over the parsed AST, never the raw text, so a comment or a
docstring that names a retired construct counts nothing.  Reads both files as
source; imports neither.  Runner: tests/infra/test_prd_capability_check.sh.
"""

import ast
import os
import re
import unittest

from _pcc_test_support import _REPO_ROOT

_ALPHA = os.path.join(_REPO_ROOT, "scripts", "prd-capability-check.py")
_PDV = os.path.join(_REPO_ROOT, "scripts", "prd-decompose-verify.py")

# The one function allowed to read tree-sitter's load-failure signature.
_BOUNDARY_PARSER = "grammar_load_failure"
_SIGNATURE_CONSTANTS = frozenset({"_GRAMMAR_LOAD_FAILURE_MARKER", "_PERMISSION_DENIAL_RE"})

# pdv's classifier, whose call closure must read no stderr.
_CLASSIFIER = "classify_record"

_SENTINEL_NAME = "_SENTINEL"
_HARNESS_ERROR_TEXT = "[HARNESS ERROR"
_ENOENT_TEXT = re.compile(r"(?i)os error|errno|enoent|no such file or directory")
_RE_PATTERN_FUNCTIONS = frozenset(
    {"compile", "search", "match", "fullmatch", "findall", "finditer", "sub", "subn", "split"}
)


def _parse(path):
    with open(path, encoding="utf-8") as fh:
        return ast.parse(fh.read(), filename=path)


def _docstring_nodes(tree):
    """The ids of the str Constant nodes that are module/class/function docstrings."""
    owners = (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)
    found = set()
    for node in ast.walk(tree):
        if isinstance(node, owners) and node.body:
            first = node.body[0]
            if (isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant)
                    and isinstance(first.value.value, str)):
                found.add(id(first.value))
    return found


def _identifiers(tree):
    """(line, identifier) for every name the code binds or reads."""
    for node in ast.walk(tree):
        if isinstance(node, ast.Name):
            yield node.lineno, node.id
        elif isinstance(node, ast.Attribute):
            yield node.lineno, node.attr
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            yield node.lineno, node.name
        elif isinstance(node, ast.arg):
            yield node.lineno, node.arg
        elif isinstance(node, ast.keyword) and node.arg:
            yield node.lineno, node.arg
        elif isinstance(node, ast.alias):
            yield node.lineno, node.name
            if node.asname:
                yield node.lineno, node.asname


def _code_strings(tree):
    """(line, text) for every str constant that is code, not a docstring."""
    docstrings = _docstring_nodes(tree)
    for node in ast.walk(tree):
        if (isinstance(node, ast.Constant) and isinstance(node.value, str)
                and id(node) not in docstrings):
            yield node.lineno, node.value


def _module_string_bindings(tree):
    """NAME → text for each module-level `NAME = "literal"` binding."""
    bound = {}
    for stmt in tree.body:
        if isinstance(stmt, ast.Assign):
            targets, value = stmt.targets, stmt.value
        elif isinstance(stmt, ast.AnnAssign) and stmt.value is not None:
            targets, value = [stmt.target], stmt.value
        else:
            continue
        if isinstance(value, ast.Constant) and isinstance(value.value, str):
            for target in targets:
                if isinstance(target, ast.Name):
                    bound[target.id] = value.value
    return bound


def _text_matchers(tree):
    """(line, text) for each literal a regex call or an `in` test matches on.

    A literal routed through a module-level constant still counts, so the
    pattern cannot hide behind a name.
    """
    bound = _module_string_bindings(tree)

    def literal(node):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            return node.value
        if isinstance(node, ast.Name):
            return bound.get(node.id)
        return None

    for node in ast.walk(tree):
        if (isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)
                and isinstance(node.func.value, ast.Name) and node.func.value.id == "re"
                and node.func.attr in _RE_PATTERN_FUNCTIONS and node.args):
            text = literal(node.args[0])
        elif (isinstance(node, ast.Compare)
                and any(isinstance(op, (ast.In, ast.NotIn)) for op in node.ops)):
            text = literal(node.left)
        else:
            continue
        if text is not None:
            yield node.lineno, text


def _call_closure(tree, root):
    """The module-level functions `root` reaches by direct call, root included."""
    functions = {
        node.name: node for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
    }
    reached, pending = {}, [root]
    while pending:
        name = pending.pop()
        if name in reached or name not in functions:
            continue
        reached[name] = functions[name]
        pending.extend(
            node.func.id for node in ast.walk(functions[name])
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
        )
    return reached


def _stderr_reads(tree, functions):
    """(function, line) for each read of a run's stderr inside `functions`.

    A read is a `"stderr"` key (subscript or .get), a `.stderr` attribute, or
    a variable named stderr; docstrings are not reads.
    """
    docstrings = _docstring_nodes(tree)
    for name, function in functions.items():
        for node in ast.walk(function):
            if (isinstance(node, ast.Constant) and node.value == "stderr"
                    and id(node) not in docstrings):
                yield name, node.lineno
            elif isinstance(node, ast.Attribute) and node.attr == "stderr":
                yield name, node.lineno
            elif isinstance(node, ast.Name) and node.id == "stderr":
                yield name, node.lineno


def _enclosing_functions(tree):
    """Map id(node) → the names of the FunctionDefs enclosing it, outermost first."""
    chains = {}

    def visit(node, chain):
        chains[id(node)] = chain
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            chain = chain + (node.name,)
        for child in ast.iter_child_nodes(node):
            visit(child, chain)

    visit(tree, ())
    return chains


def _sentinel_names(tree):
    return [(line, name) for line, name in _identifiers(tree) if _SENTINEL_NAME in name]


def _harness_error_producers(tree):
    return [(line, text) for line, text in _code_strings(tree) if _HARNESS_ERROR_TEXT in text]


def _enoent_matchers(tree):
    return [(line, text) for line, text in _text_matchers(tree) if _ENOENT_TEXT.search(text)]


class TestRatchetCountsCodeNotProse(unittest.TestCase):
    """The detectors below the ratchet see code constructs and nothing else.

    Each retired construct appears here once as code and once as prose (a
    comment and a docstring), so a detector that read raw text, or one that
    saw nothing at all, would miscount.
    """

    _SAMPLE = '''
"""Module docstring: _OLD_SENTINEL, [HARNESS ERROR, os error 2, rec["stderr"]."""
import re
# A comment: _OLD_SENTINEL, "[HARNESS ERROR", errno, No such file or directory.
_LIVE_SENTINEL = "[HARNESS ERROR: launch]"
_PHRASE = "No such file or directory"
_ERRNO_RE = re.compile("os error 2")
_PARSE_RE = re.compile("Failed to parse")


def reads_stderr(rec):
    """Docstring: never reads stderr."""
    return helper(rec)


def helper(rec):
    return _PHRASE in rec.get("stderr", "")


def reads_fields(rec):
    """Docstring: rec["stderr"] would be a read."""
    return rec.get("fixture_present") is False
'''

    def setUp(self):
        self.tree = ast.parse(self._SAMPLE)

    def test_sentinel_names_are_identifiers_only(self):
        self.assertEqual([n for _, n in _sentinel_names(self.tree)], ["_LIVE_SENTINEL"])

    def test_harness_error_producers_are_code_strings_only(self):
        self.assertEqual(
            [t for _, t in _harness_error_producers(self.tree)], ["[HARNESS ERROR: launch]"]
        )

    def test_enoent_matchers_are_regex_and_containment_literals_only(self):
        self.assertEqual(
            sorted(t for _, t in _enoent_matchers(self.tree)),
            ["No such file or directory", "os error 2"],
            "the bare _PHRASE binding is not a matcher; its use in `in` is",
        )

    def test_stderr_reads_follow_the_call_closure(self):
        closure = _call_closure(self.tree, "reads_stderr")
        self.assertEqual(sorted(closure), ["helper", "reads_stderr"])
        self.assertEqual([f for f, _ in _stderr_reads(self.tree, closure)], ["helper"])
        fields_only = _call_closure(self.tree, "reads_fields")
        self.assertEqual(list(_stderr_reads(self.tree, fields_only)), [])


class TestTypedOutcomeRatchet(unittest.TestCase):
    """Counts of the superseded inline patterns, held at zero."""

    _FILES = {"prd-capability-check.py": _ALPHA, "prd-decompose-verify.py": _PDV}

    def test_r1_no_stderr_sentinel_names(self):
        for name, path in self._FILES.items():
            with self.subTest(file=name):
                found = _sentinel_names(_parse(path))
                self.assertEqual(
                    found, [],
                    f"{name} names a '{_SENTINEL_NAME}' identifier {len(found)} "
                    f"time(s) (line, name): {found!r}; a harness failure is "
                    "HarnessFailure on the run, never a marker in stderr",
                )

    def test_r2_no_harness_error_text_producers(self):
        for name, path in self._FILES.items():
            with self.subTest(file=name):
                found = _harness_error_producers(_parse(path))
                self.assertEqual(
                    found, [],
                    f"{name} carries {len(found)} '{_HARNESS_ERROR_TEXT}' string "
                    f"literal(s) (line, text): {found!r}; stderr holds only what "
                    "the tool printed",
                )

    def test_r3_pdv_matches_no_errno_text(self):
        found = _enoent_matchers(_parse(_PDV))
        self.assertEqual(
            found, [],
            f"prd-decompose-verify.py matches ENOENT/errno text {len(found)} "
            f"time(s) in a regex or `in` test (line, literal): {found!r}; "
            "fixture absence is α's fixture_present field",
        )

    def test_r3b_pdv_classifier_reads_no_stderr(self):
        tree = _parse(_PDV)
        closure = _call_closure(tree, _CLASSIFIER)
        self.assertIn(
            _CLASSIFIER, closure,
            f"prd-decompose-verify.py defines no {_CLASSIFIER}(), so this check "
            "would be vacuous",
        )
        reads = list(_stderr_reads(tree, closure))
        self.assertEqual(
            reads, [],
            f"{_CLASSIFIER}() or a function it calls reads stderr {len(reads)} "
            f"time(s) (function, line): {reads!r}; it classifies from α's typed "
            "fields only",
        )

    def test_r4_one_reader_of_the_tree_sitter_signature(self):
        readers = {}
        for name, path in self._FILES.items():
            tree = _parse(path)
            chains = _enclosing_functions(tree)
            readers[name] = [
                (node.id, chains[id(node)]) for node in ast.walk(tree)
                if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Load)
                and node.id in _SIGNATURE_CONSTANTS
            ]
        self.assertTrue(
            readers["prd-capability-check.py"],
            "no read of the tree-sitter signature constants was found at all, so "
            "this check would be vacuous",
        )
        self.assertEqual(
            readers["prd-decompose-verify.py"], [],
            "prd-decompose-verify.py must not read tree-sitter's signature",
        )
        strays = [
            (constant, chain) for constant, chain in readers["prd-capability-check.py"]
            if chain != (_BOUNDARY_PARSER,)
        ]
        self.assertEqual(
            strays, [],
            f"prd-capability-check.py reads the tree-sitter signature outside "
            f"{_BOUNDARY_PARSER}() {len(strays)} time(s): {strays!r} "
            "(constant, enclosing functions)",
        )


if __name__ == "__main__":
    unittest.main()
