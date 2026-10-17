#!/usr/bin/env python3
"""
test_pcc_outcome_ratchet.py — the INV-5 adoption ratchet for the typed probe
outcome (docs/prds/v0_6/prd-probe-gate-typed-outcomes.md D1/D2, §8 γ).  How a
probe's machinery failed and whether its fixture existed are typed fields, so
scripts/prd-capability-check.py and scripts/prd-decompose-verify.py hold no
stderr sentinel, no ENOENT text route, and exactly one reader of tree-sitter's
load-failure signature.  Reads both files as source; imports neither.  Runner:
tests/infra/test_prd_capability_check.sh.
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

_ENOENT_TEXT = re.compile(r"(?i)os error|errno|no such file or directory")


def _source(path):
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def _docstring_nodes(tree):
    """The str Constant nodes that are module/class/function docstrings."""
    owners = (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)
    found = set()
    for node in ast.walk(tree):
        if isinstance(node, owners) and node.body:
            first = node.body[0]
            if (isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant)
                    and isinstance(first.value.value, str)):
                found.add(id(first.value))
    return found


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


class TestTypedOutcomeRatchet(unittest.TestCase):
    """Counts of the superseded inline patterns, held at zero."""

    _FILES = {"prd-capability-check.py": _ALPHA, "prd-decompose-verify.py": _PDV}

    def test_r1_no_stderr_sentinel_names(self):
        for name, path in self._FILES.items():
            with self.subTest(file=name):
                count = _source(path).count("_SENTINEL")
                self.assertEqual(
                    count, 0,
                    f"{name} mentions '_SENTINEL' {count} time(s); a harness failure "
                    "is HarnessFailure on the run, never a marker in stderr",
                )

    def test_r2_no_harness_error_text(self):
        for name, path in self._FILES.items():
            with self.subTest(file=name):
                count = _source(path).count("[HARNESS ERROR")
                self.assertEqual(
                    count, 0,
                    f"{name} carries '[HARNESS ERROR' {count} time(s); stderr holds "
                    "only what the tool printed",
                )

    def test_r3_pdv_routes_on_no_enoent_text(self):
        tree = ast.parse(_source(_PDV))
        docstrings = _docstring_nodes(tree)
        literals = [
            node.value for node in ast.walk(tree)
            if isinstance(node, ast.Constant) and isinstance(node.value, str)
            and id(node) not in docstrings and _ENOENT_TEXT.search(node.value)
        ]
        self.assertEqual(
            literals, [],
            f"prd-decompose-verify.py holds {len(literals)} ENOENT/errno string "
            f"literal(s) {literals!r}; fixture absence is α's fixture_present field",
        )

    def test_r4_one_reader_of_the_tree_sitter_signature(self):
        readers = {}
        for name, path in self._FILES.items():
            tree = ast.parse(_source(path))
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
