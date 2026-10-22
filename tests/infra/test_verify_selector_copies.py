#!/usr/bin/env python3
"""
test_verify_selector_copies.py — scripts/reify_verify/selection.py's copies of
reify-audit's ptodo, pdiag and pdoccover predicates select what the Rust
source reads (S18 β, task 9097; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md §2.3, §8, D13).

Every check drives a public seam as a subprocess, `python3 -I -B
scripts/verify.py ... --print-scope`, in B1's shared clone; this module never
imports reify_verify. The corpora are derived from the Rust source by
tests/infra/reify_audit_inputs.py. A reify-audit predicate edit selects this
member alone, not the narrowing and selection parity suite.

The shared clone and the hermetic env live in
tests/infra/scope_record_harness.py.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_selector_copies.sh.
"""

import sys
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import reify_audit_inputs  # noqa: E402
from scope_record_harness import (  # noqa: E402
    STAGED_SCOPE,
    InClone,
    discard_clone,
    leaf_globs,
    scope_record,
)

PTODO_LEAF = "tests/infra/test_reify_audit_ptodo.sh"
PDIAG_LEAF = "tests/infra/test_reify_audit_pdiag.sh"
PDOCCOVER_LEAF = "tests/infra/test_reify_audit_pdoccover.sh"


def tearDownModule():
    discard_clone()


class TestSelectorCopiesMatchReifyAudit(InClone):
    """selection.py's copies of reify-audit's predicates select what the Rust source reads.

    The corpora are derived as test_verify_scope.sh's PT-DRIFT, PDIAG-DRIFT
    and PDOCCOVER-DRIFT derive them for bash's copies, so neither copy can
    drift from ptodo.rs, pdiag.rs or pdoccover.rs alone. Each path that must
    select a leaf runs alone; the paths that must not run together, since any
    one of them selecting it is the failure.
    """

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.clone.reset()
        cls.extensions = reify_audit_inputs.ptodo_swept_extensions(cls.clone.root)
        cls.pdiag = reify_audit_inputs.pdiag_corpus(cls.clone.root)
        cls.pdoccover = reify_audit_inputs.pdoccover_inputs(cls.clone.root)

    def test_premise_the_corpora_are_not_vacuous(self):
        self.assertTrue(self.extensions, "is_swept_ext yielded no extension")
        self.assertTrue(self.pdiag.swept and self.pdiag.not_swept, self.pdiag)
        self.assertEqual(self.pdiag.read_calls, self.pdiag.test_calls, "an is_swept_path test the scrape cannot read")
        self.assertTrue(self.pdiag.excluded_prefixes, self.pdiag)
        self.assertTrue(self.pdoccover.roots, self.pdoccover)
        self.assertEqual(len(self.pdoccover.exact), 2, self.pdoccover)
        self.assertEqual(
            self.pdoccover.oracle_pairs, self.pdoccover.oracle_starts, "an in_oracle_scope root the scrape cannot read"
        )

    def test_every_swept_extension_selects_the_ptodo_gate(self):
        paths = [f"docs/pt_probe.{extension}" for extension in self.extensions]
        for path, outcome in self.clone.read_each_staged_alone(paths, STAGED_SCOPE).items():
            with self.subTest(path=path):
                self.assertIn(PTODO_LEAF, leaf_globs(scope_record(outcome)))

    def test_is_swept_path_corpus_selects_the_pdiag_ratchet(self):
        for path in self.pdiag.swept:
            with self.subTest(swept=path):
                self.assertIn(PDIAG_LEAF, self.clone.branch_leaves([path]))
        refused = [
            *self.pdiag.not_swept,
            *(f"{prefix}src/pdiag_drift_probe.rs" for prefix in self.pdiag.excluded_prefixes),
        ]
        self.assertNotIn(
            PDIAG_LEAF, self.clone.branch_leaves(refused), f"one of {refused} selected the PDIAG ratchet"
        )

    def test_every_pdoccover_input_selects_the_pdoccover_ratchet(self):
        inputs = [
            *(f"{prefix}pdoccover_drift/probe{suffix}" for _, prefix, suffix in self.pdoccover.roots),
            *(path for _, path in self.pdoccover.exact),
        ]
        for path in inputs:
            with self.subTest(input=path):
                self.assertIn(PDOCCOVER_LEAF, self.clone.branch_leaves([path]))
        refused = [
            *(f"{prefix}pdoccover_drift_probe{suffix}.orig" for _, prefix, suffix in self.pdoccover.roots),
            *(f"{path}.orig" for _, path in self.pdoccover.exact),
        ]
        self.assertNotIn(
            PDOCCOVER_LEAF, self.clone.branch_leaves(refused), f"one of {refused} selected the PDOCCOVER ratchet"
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
