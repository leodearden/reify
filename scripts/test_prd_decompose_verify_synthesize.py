#!/usr/bin/env python3
"""
test_prd_decompose_verify_synthesize.py — synthesize_batch(), evidence-gate, fixture-absent
and report-format tests for scripts/prd-decompose-verify.py.
"""

import io
import json
import os
import sys
import tempfile
import unittest
import unittest.mock
from typing import Any

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _pdv_test_support import PDV_MJS, load_pdv, reify_missing_fixture_stderr

pdv = load_pdv()

_LEAF_FIXTURE = "tests/prd-gate/fixtures/leaf.ri"


# ---------------------------------------------------------------------------
# step-05 (RED): synthesize_batch() blocking semantics + captured-output mandate
# ---------------------------------------------------------------------------

class TestSynthesizeBatch(unittest.TestCase):
    """Tests for BatchVerdict + synthesize_batch() blocking semantics.

    Uses synthetic α --json result records.
    These tests FAIL until step-06 implements BatchVerdict + synthesize_batch.
    """

    def _result(self, capability: str, verdict: str,
                exit_code: int = 0, stdout: str = "", stderr: str = "") -> dict:
        """Build a synthetic α --json result record."""
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["reify", "check", "/fixture.ri"],
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }

    # ── (1) all-PASS prover + empty adversary → does not block ───────────────

    def test_all_pass_prover_empty_adversary_does_not_block(self):
        """All PASS prover results + empty adversary → blocks==False."""
        role_results = {
            "prover": [
                self._result("cap-A", "PASS"),
                self._result("cap-B", "PASS"),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertFalse(bv.blocks)

    def test_all_pass_prover_and_adversary_does_not_block(self):
        """All PASS in both prover and adversary → blocks==False."""
        role_results = {
            "prover": [self._result("cap-A", "PASS")],
            "adversary": [self._result("cap-B", "PASS")],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertFalse(bv.blocks)

    # ── (2) prover FAIL → blocks + capability listed ─────────────────────────

    def test_prover_fail_blocks(self):
        """Any prover FAIL → blocks==True."""
        role_results = {
            "prover": [
                self._result("cap-A", "PASS"),
                self._result("cap-B", "FAIL", exit_code=0, stdout="All constraints satisfied."),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_prover_fail_lists_failing_capability(self):
        """prover FAIL → failing capability name is in bv.blocking."""
        role_results = {
            "prover": [
                self._result("cap-FAILING", "FAIL",
                             exit_code=0, stdout="All constraints satisfied."),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("cap-FAILING", bv.blocking)

    # ── (3) UNPROVABLE → blocks ───────────────────────────────────────────────

    def test_prover_unprovable_blocks(self):
        """Any UNPROVABLE → blocks==True."""
        role_results = {
            "prover": [self._result("cap-U", "UNPROVABLE", exit_code=1, stderr="unrelated err")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_prover_unprovable_listed(self):
        """UNPROVABLE capability is listed in bv.blocking."""
        role_results = {
            "prover": [self._result("cap-U", "UNPROVABLE", exit_code=1)],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("cap-U", bv.blocking)

    # ── (4) HARNESS_ERROR → blocks ───────────────────────────────────────────

    def test_prover_harness_error_blocks(self):
        """Any HARNESS_ERROR → blocks==True."""
        role_results = {
            "prover": [self._result("cap-HE", "HARNESS_ERROR")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_prover_harness_error_listed(self):
        """HARNESS_ERROR capability is listed in bv.blocking."""
        role_results = {
            "prover": [self._result("cap-HE", "HARNESS_ERROR")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("cap-HE", bv.blocking)

    # ── (5) adversary-only FAIL (falsification / unlisted premise) → blocks ───

    def test_adversary_only_fail_blocks(self):
        """Adversary-only FAIL (unlisted premise / falsification) → blocks==True."""
        role_results = {
            "prover": [self._result("cap-A", "PASS")],
            "adversary": [self._result("cap-ADVERSARY-FAIL", "FAIL",
                                       exit_code=0, stdout="")],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_adversary_fail_listed_in_blocking(self):
        """Adversary FAIL capability appears in bv.blocking."""
        role_results = {
            "prover": [],
            "adversary": [self._result("adv-FAIL-cap", "FAIL", exit_code=0)],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("adv-FAIL-cap", bv.blocking)

    # ── (6) adversary PASS never clears a prover FAIL ─────────────────────────

    def test_adversary_pass_cannot_clear_prover_fail(self):
        """Adversary PASS never clears a prover FAIL (net-positive recall)."""
        role_results = {
            "prover": [self._result("cap-B", "FAIL",
                                    exit_code=0, stdout="All constraints satisfied.")],
            "adversary": [self._result("cap-B", "PASS", exit_code=1)],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks,
                        "Adversary PASS must not clear a prover FAIL — it can only add signals")

    # ── (7) report contains captured evidence for blocking probes ─────────────

    def test_report_contains_command_for_failing_probe(self):
        """bv.report includes the exact command for each blocking probe."""
        role_results = {
            "prover": [
                self._result("cap-FAIL", "FAIL",
                             exit_code=0, stdout="All constraints satisfied.", stderr=""),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        # The command is ["reify", "check", "/fixture.ri"] — at least "reify" must appear
        self.assertIn("reify", bv.report)

    def test_report_contains_exit_code_for_failing_probe(self):
        """bv.report includes the labelled exit_code line for each blocking probe."""
        role_results = {
            "prover": [self._result("cap-FAIL", "FAIL", exit_code=0, stdout="x")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        # The labelled "exit_code: 0" line must appear so the assertion can't pass
        # simply because the digit '0' appears elsewhere (e.g. in timestamps or names).
        self.assertIn("exit_code: 0", bv.report)

    def test_report_contains_stdout_for_failing_probe(self):
        """bv.report includes the captured stdout for each blocking probe."""
        role_results = {
            "prover": [
                self._result("cap-FAIL", "FAIL",
                             exit_code=0, stdout="All constraints satisfied.", stderr="")
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("All constraints satisfied.", bv.report)

    def test_report_contains_stderr_for_failing_probe(self):
        """bv.report includes the captured stderr for each blocking probe."""
        role_results = {
            "prover": [
                self._result("cap-FAIL", "FAIL",
                             exit_code=1, stdout="", stderr="error: type mismatch")
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("error: type mismatch", bv.report)

    # ── (8) BatchVerdict has required fields ──────────────────────────────────

    def test_batch_verdict_has_blocks_field(self):
        """BatchVerdict has a .blocks bool field."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertIsInstance(bv.blocks, bool)

    def test_batch_verdict_has_blocking_list(self):
        """BatchVerdict has a .blocking list field."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertIsInstance(bv.blocking, list)

    def test_batch_verdict_has_report_string(self):
        """BatchVerdict has a .report string field."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertIsInstance(bv.report, str)

    def test_blocking_is_empty_when_all_pass(self):
        """bv.blocking is empty when nothing blocks."""
        bv = pdv.synthesize_batch({"prover": [self._result("cap", "PASS")], "adversary": []})
        self.assertEqual(bv.blocking, [])


# ---------------------------------------------------------------------------
# task #7257 step-01 (RED): command normalization (ARM 1, item 3)
# ---------------------------------------------------------------------------

class TestCommandNormalization(unittest.TestCase):
    """`command` may arrive as a string; rendering it must not explode it.

    Observed on this branch before the fix: a record carrying
    `"command": "target/release/reify eval f.ri"` (a STRING, not a list) was
    rendered by synthesize_batch's `" ".join(rec.get("command", []))` as
    `t a r g e t / r e l e a s e / r e i f y   e v a l   f . r i` — Python
    joins a string character-by-character.  The captured evidence a human is
    meant to re-run became unreadable.

    GREEN in task #7257 step-02 (pdv.normalize_command).
    """

    _STRING_CMD = "target/release/reify eval f.ri"
    _EXPLODED = "t a r g e t"

    # ── (a) unit tests for normalize_command ─────────────────────────────────

    def test_list_round_trips_as_list_of_str(self):
        """A list of strings round-trips unchanged."""
        self.assertEqual(
            pdv.normalize_command(["reify", "check", "/fixture.ri"]),
            ["reify", "check", "/fixture.ri"],
        )

    def test_list_items_are_stringified(self):
        """Non-str items in a list are coerced to str (evidence stays renderable)."""
        self.assertEqual(pdv.normalize_command(["reify", 7, None]), ["reify", "7", "None"])

    def test_tuple_becomes_list(self):
        """A tuple normalizes to a list (JSON round-trips give lists, tests give tuples)."""
        out = pdv.normalize_command(("reify", "check"))
        self.assertIsInstance(out, list)
        self.assertEqual(out, ["reify", "check"])

    def test_string_becomes_single_element_list(self):
        """A STRING command becomes ONE token, so `" ".join` renders it verbatim."""
        self.assertEqual(pdv.normalize_command(self._STRING_CMD), [self._STRING_CMD])

    def test_none_becomes_empty_list(self):
        """None (absent command) normalizes to []."""
        self.assertEqual(pdv.normalize_command(None), [])

    def test_int_becomes_empty_list(self):
        """A non-str, non-sequence value carries no command evidence → []."""
        self.assertEqual(pdv.normalize_command(7), [])

    def test_empty_list_stays_empty(self):
        """An explicit empty list stays empty (no evidence)."""
        self.assertEqual(pdv.normalize_command([]), [])

    # ── (b) end-to-end through synthesize_batch ──────────────────────────────

    def _string_command_record(self) -> dict:
        """A blocking record whose `command` is a STRING rather than a list."""
        return {
            "capability": "string-command capability",
            "probe_kind": "ir",
            "verdict": "FAIL",
            "command": self._STRING_CMD,
            "exit_code": 1,
            "stdout": "",
            "stderr": "assertion did not hold",
        }

    def test_string_command_renders_verbatim_in_report(self):
        """The report shows the command exactly as captured."""
        bv = pdv.synthesize_batch({"prover": [self._string_command_record()], "adversary": []})
        self.assertIn(
            self._STRING_CMD, bv.report,
            f"string command must render verbatim; report was:\n{bv.report}",
        )

    def test_string_command_is_not_character_exploded(self):
        """The report must NOT contain the character-spaced explosion."""
        bv = pdv.synthesize_batch({"prover": [self._string_command_record()], "adversary": []})
        self.assertNotIn(
            self._EXPLODED, bv.report,
            f"string command was exploded character-by-character; report was:\n{bv.report}",
        )


# ---------------------------------------------------------------------------
# task #7257 step-03 (RED): evidence gate for blocking records (ARM 1, item 1)
# ---------------------------------------------------------------------------

class TestEvidenceGate(unittest.TestCase):
    """A blocking verdict with no executed-probe evidence is a harness defect.

    PRD §6 decision 4: "Captured output is mandatory on every verdict. […]
    Every D1 result carries the exact command + stdout/stderr + exit code, so a
    human (or D4) can re-derive the verdict without re-running."  An
    evidence-free record is therefore not a valid falsification at all — it is
    an unexecuted promise, and tabulating it as `blocking` buries the ONE real
    finding among N vacuous ones (the observed pi-report failure).

    GREEN in task #7257 step-04 (has_probe_evidence / classify_record).
    """

    def _result(self, capability: str, verdict: str, *,
                command: Any = ("reify", "check", "/fixture.ri"),
                exit_code: Any = 1,
                stdout: str = "", stderr: str = "",
                omit_command: bool = False,
                omit_exit_code: bool = False) -> dict:
        """Build a synthetic α --json result record, with omissions on request."""
        rec: dict = {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": list(command) if isinstance(command, tuple) else command,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }
        if omit_command:
            del rec["command"]
        if omit_exit_code:
            del rec["exit_code"]
        return rec

    # ── (1)(2) evidence-free FAIL: not blocking, but named as malformed ──────

    def test_evidence_free_fail_does_not_block(self):
        """A FAIL with command [] and exit_code None must not appear in blocking."""
        rec = self._result("evidence-free cap", "FAIL", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertNotIn("evidence-free cap", bv.blocking)
        self.assertFalse(bv.blocks, "an unexecuted promise must not block the batch")

    def test_evidence_free_fail_is_reported_as_malformed(self):
        """The same record's capability IS surfaced — as malformed, not blocking."""
        rec = self._result("evidence-free cap", "FAIL", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("evidence-free cap", bv.malformed)

    # ── (3)(4) partial evidence is still no evidence ─────────────────────────

    def test_missing_command_key_is_malformed(self):
        """A record with no `command` key at all → malformed, not blocking."""
        rec = self._result("no-command cap", "FAIL", omit_command=True)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("no-command cap", bv.malformed)
        self.assertNotIn("no-command cap", bv.blocking)

    def test_missing_exit_code_key_is_malformed(self):
        """A non-empty command but no `exit_code` key → no process outcome → malformed."""
        rec = self._result("no-exit-code cap", "UNPROVABLE", omit_exit_code=True)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("no-exit-code cap", bv.malformed)
        self.assertNotIn("no-exit-code cap", bv.blocking)

    def test_exit_code_zero_is_valid_evidence(self):
        """exit_code 0 is a real outcome — `is not None`, not truthiness."""
        rec = self._result("exit-zero cap", "FAIL", exit_code=0)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("exit-zero cap", bv.blocking)
        self.assertEqual(bv.malformed, [])

    # ── (5) the headline signal: one real finding is no longer buried ────────

    def test_one_executed_fail_among_evidence_free_fails_is_the_only_blocker(self):
        """The real finding stands alone; the three vacuous ones do not dilute it."""
        role_results = {
            "prover": [
                self._result("vacuous-1", "FAIL", command=[], exit_code=None),
                self._result("REAL executed fail", "FAIL", exit_code=1,
                             stderr="type mismatch: expected axis"),
                self._result("vacuous-2", "FAIL", omit_command=True),
            ],
            "adversary": [
                self._result("vacuous-3", "UNPROVABLE", command=[], omit_exit_code=True),
            ],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)
        self.assertEqual(bv.blocking, ["REAL executed fail"])
        self.assertEqual(sorted(bv.malformed), ["vacuous-1", "vacuous-2", "vacuous-3"])

    # ── (6) existing blocking semantics preserved for executed probes ────────

    def test_executed_unprovable_still_blocks(self):
        """An executed UNPROVABLE keeps blocking (unchanged semantics)."""
        rec = self._result("unprovable cap", "UNPROVABLE", exit_code=2)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("unprovable cap", bv.blocking)

    def test_executed_harness_error_still_blocks(self):
        """An executed HARNESS_ERROR keeps blocking (unchanged semantics)."""
        rec = self._result("harness-error cap", "HARNESS_ERROR", exit_code=-1,
                           stderr="probe runner crashed")
        bv = pdv.synthesize_batch({"prover": [], "adversary": [rec]})
        self.assertTrue(bv.blocks)
        self.assertIn("harness-error cap", bv.blocking)

    # ── (7) a PREMISE-shaped record is neither blocking nor malformed ────────

    def test_premise_shaped_record_is_neither_blocking_nor_malformed(self):
        """RESULTS_SCHEMA is loose enough today that a premise validates as a result.

        Such a record has no `verdict` key at all.  It is not a falsification
        and it is not an evidence-free BLOCKING verdict either — the evidence
        gate must not manufacture a malformed entry out of it.
        """
        premise_shaped = {
            "capability": "a revolute joint rejects a non-axis argument",
            "assertion_kind": "rejection",
            "fixture": "tests/prd-gate/fixtures/revolute-non-axis.ri",
        }
        bv = pdv.synthesize_batch({"prover": [premise_shaped], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.blocking, [])
        self.assertEqual(bv.malformed, [])

    def test_pass_record_is_not_malformed_even_without_evidence(self):
        """The gate applies to BLOCKING verdicts only; a PASS is not re-litigated."""
        rec = self._result("passing cap", "PASS", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.malformed, [])
        self.assertEqual(bv.blocking, [])

    # ── (8) executed / total counters ────────────────────────────────────────

    def test_executed_and_total_counters_on_a_mixed_set(self):
        """`executed` counts records with probe evidence; `total` counts all records."""
        role_results = {
            "prover": [
                self._result("p-pass", "PASS", exit_code=0),
                self._result("p-fail", "FAIL", exit_code=1),
                self._result("p-vacuous", "FAIL", command=[], exit_code=None),
            ],
            "adversary": [
                self._result("a-pass", "PASS", exit_code=0),
                self._result("a-vacuous", "UNPROVABLE", omit_command=True),
            ],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertEqual(bv.total, 5)
        self.assertEqual(bv.executed, 3)

    def test_counters_are_zero_on_an_empty_batch(self):
        """An empty batch reports 0 executed of 0 total (not a silent pass basis)."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertEqual(bv.total, 0)
        self.assertEqual(bv.executed, 0)

    def test_malformed_report_section_names_the_capability(self):
        """A malformed record is visible in the report, labelled as a harness defect."""
        rec = self._result("evidence-free cap", "FAIL", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("evidence-free cap", bv.report)
        self.assertIn("MALFORMED", bv.report)


# ---------------------------------------------------------------------------
# fixture-absent ≠ falsification, decided by α's typed field (PRD §5.2, D3)
# ---------------------------------------------------------------------------

class TestFixtureAbsent(unittest.TestCase):
    """A probe whose fixture did not exist has falsified nothing.

    During decompose the .ri fixture a premise probes is very often the leaf's
    own deliverable, so it does not exist yet.  α measures that when the probe
    launches and records it as `fixture_present`; pdv routes on that field and
    on nothing else, so a FAIL whose stderr merely quotes ENOENT keeps blocking
    and a relay that drops or mangles the field can never stop a block.
    """

    _ENOENT = reify_missing_fixture_stderr(_LEAF_FIXTURE)

    def _record(self, capability="cap", verdict="FAIL", stderr="", evidence=True,
                **fields):
        rec = {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["reify", "check", _LEAF_FIXTURE] if evidence else [],
            "exit_code": 1 if evidence else None,
            "stdout": "",
            "stderr": stderr,
        }
        rec.update(fields)
        return rec

    def _synthesize(self, rec):
        return pdv.synthesize_batch({"prover": [rec], "adversary": []})

    # ── (1) the absent-fixture downgrade, and how it is reported ─────────────

    def test_absent_fixture_sets_a_blocking_verdict_aside(self):
        for verdict in ("FAIL", "UNPROVABLE"):
            with self.subTest(verdict=verdict):
                cap = f"{verdict} on a missing fixture"
                rec = self._record(cap, verdict, fixture_present=False)
                self.assertEqual(pdv.classify_record(rec), pdv.CAT_FIXTURE_ABSENT)
                bv = self._synthesize(rec)
                self.assertFalse(bv.blocks)
                self.assertEqual(bv.blocking, [])
                self.assertEqual(bv.fixture_absent, [cap])
                self.assertEqual(bv.malformed, [])
                self.assertEqual((bv.executed, bv.total), (1, 1),
                                 "the probe DID run; it had no target")
                _, section = bv.report.split("FIXTURE ABSENT", 1)
                self.assertIn(cap, section)

    # ── (2) the PRD §5.2 precedence, one row per rule it orders ──────────────

    def test_classification_follows_the_prd_precedence(self):
        cases = (
            ("a relayed PASS on a missing fixture (M1)",
             self._record(verdict="PASS", fixture_present=False),
             pdv.CAT_FIXTURE_ABSENT),
            ("a harness failure outranks the absence",
             self._record(verdict="HARNESS_ERROR", exit_code=127,
                          fixture_present=False,
                          harness_failure={"kind": "launch",
                                           "detail": "[Errno 2] No such file or "
                                                     "directory: 'reify'",
                                           "cache_denied": False}),
             pdv.CAT_BLOCKING),
            ("no executed evidence outranks the absence",
             self._record(evidence=False, fixture_present=False),
             pdv.CAT_MALFORMED),
            ("B3: a present fixture whose FAIL quotes ENOENT on its own line",
             self._record(stderr=f'{_LEAF_FIXTURE}:3: expected '
                                 '"No such file or directory (os error 2)"',
                          fixture_present=True),
             pdv.CAT_BLOCKING),
            ("#8122: tree-sitter's missing-file report",
             self._record(probe_kind="grammar",
                          command=["tree-sitter", "parse", "--quiet", _LEAF_FIXTURE],
                          stderr="Error: No files were found at or matched by the "
                                 "provided pathname/glob",
                          fixture_present=False),
             pdv.CAT_FIXTURE_ABSENT),
        )
        for label, rec, category in cases:
            with self.subTest(case=label):
                self.assertEqual(pdv.classify_record(rec), category)

    def test_harness_failure_on_a_missing_fixture_still_blocks(self):
        rec = self._record("launch cap", "HARNESS_ERROR", exit_code=127,
                           fixture_present=False,
                           harness_failure={"kind": "launch", "detail": "x",
                                            "cache_denied": False})
        bv = self._synthesize(rec)
        self.assertTrue(bv.blocks, "the machinery failing is the louder fact")
        self.assertEqual(bv.blocking, ["launch cap"])
        self.assertEqual(bv.fixture_absent, [])

    # ── (3) B5: only α's boolean false routes; the stderr text never does ────

    def test_enoent_stderr_without_a_boolean_false_blocks(self):
        """Fail-closed (D3): missing, null and every non-bool falsy value block."""
        variants = {
            "field missing": {},
            "field null": {"fixture_present": None},
            "field 0": {"fixture_present": 0},
            "field 'false'": {"fixture_present": "false"},
        }
        for label, fields in variants.items():
            with self.subTest(variant=label):
                rec = self._record(f"{label} cap", stderr=self._ENOENT, **fields)
                self.assertEqual(pdv.classify_record(rec), pdv.CAT_BLOCKING)
                bv = self._synthesize(rec)
                self.assertTrue(bv.blocks)
                self.assertEqual(bv.fixture_absent, [])

    # ── (4) a pass on a present fixture is untouched ─────────────────────────

    def test_pass_on_a_present_fixture_is_non_blocking(self):
        for label, fields in {
            "field true": {"fixture_present": True},
            "field missing": {},
            "field null": {"fixture_present": None},
        }.items():
            with self.subTest(variant=label):
                rec = self._record(verdict="PASS", stderr=self._ENOENT, **fields)
                self.assertEqual(pdv.classify_record(rec), pdv.CAT_NON_BLOCKING)


# ---------------------------------------------------------------------------
# task #7257 amendment: HARNESS_ERROR is exempt from BOTH evidence downgrades
# ---------------------------------------------------------------------------

class TestHarnessErrorAlwaysBlocks(unittest.TestCase):
    """A HARNESS_ERROR blocks regardless of evidence, and is never fixture-absent.

    Two review findings, one root cause.  The evidence gate and the
    fixture-absent carve-out both exist to stop a PREMISE FALSIFICATION being
    claimed on no evidence.  HARNESS_ERROR makes no such claim — it reports
    that the probe machinery could not run — so routing it through either
    downgrade inverts its meaning and stops it blocking:

      - MALFORMED: the Prover prompt's own documented fallback for "I could not
        run anything at all" is `{verdict: "HARNESS_ERROR", command: [],
        exit_code: -1}`, which carries no evidence BY CONSTRUCTION.  Under a
        blanket gate, python3 missing / a wrong α path / a dead shell step
        silently became INCOMPLETE instead of BLOCKS.
      - FIXTURE_ABSENT: a harness-level ENOENT (`python3: can't open file ...
        [Errno 2] No such file or directory`) was once reported as "the fixture
        is the leaf's own deliverable" — the opposite diagnosis.

    The blast radius is pinned in both directions: an evidence-free FAIL is
    still MALFORMED, and a FAIL whose fixture α measured absent is still
    FIXTURE_ABSENT.
    """

    # The exact fallback record the Prover prompt tells the agent to emit.
    # _assert_the_mjs_prompt_still_emits_this_shape keeps it honest.
    def _prompt_fallback_record(self, capability: str = "leaf label (delta)") -> dict:
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": "HARNESS_ERROR",
            "command": [],
            "exit_code": -1,
            "stdout": "",
            "stderr": "bind step exited 2: no such option --json",
            "fixture_present": None,
        }

    def _executed(self, capability: str, verdict: str, stderr: str,
                  exit_code: int = 2) -> dict:
        """An EXECUTED record (real command, real exit code) with a chosen stderr."""
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["python3", "scripts/prd-capability-check.py",
                        "--json", "/tmp/ps.json"],
            "exit_code": exit_code,
            "stdout": "",
            "stderr": stderr,
        }

    # ── (1) the prompt's own fallback record produces the intended disposition ─

    def test_prompt_fallback_record_blocks(self):
        """The documented "nothing ran" record must still stop the batch."""
        rec = self._prompt_fallback_record()
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks,
                        "an evidence-free HARNESS_ERROR is the ONE path that means "
                        "'the harness itself died' — it must block")
        self.assertEqual(bv.blocking, ["leaf label (delta)"])

    def test_prompt_fallback_record_is_not_malformed(self):
        """It is not an unexecuted promise — nothing was promised."""
        rec = self._prompt_fallback_record()
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.malformed, [])
        self.assertEqual(bv.fixture_absent, [])

    def test_prompt_fallback_record_is_classified_blocking(self):
        """Unit-level, so a regression localises to classify_record."""
        self.assertEqual(pdv.classify_record(self._prompt_fallback_record()),
                         pdv.CAT_BLOCKING)

    def test_the_mjs_prompt_still_emits_this_shape(self):
        """Drift guard: the record above is a COPY of the .mjs prompt template.

        If the prompt's fallback stops saying `command: [], exit_code: -1`, the
        tests above stop testing the shape that actually reaches the harness.
        """
        with open(PDV_MJS, encoding="utf-8") as fh:
            src = fh.read()
        self.assertIn('verdict: "HARNESS_ERROR"', src,
                      "the Prover prompt no longer documents a HARNESS_ERROR fallback")
        self.assertIn("command: [], exit_code: -1", src,
                      "the fallback template no longer emits the evidence-free shape "
                      "these tests pin; re-sync _prompt_fallback_record")

    def test_evidence_free_harness_error_still_counts_zero_executed(self):
        """The exemption is about BLOCKING, not about faking evidence.

        `executed` must stay honest: nothing ran, so the counts header still
        reports 0 with executed-probe evidence even though the batch blocks.
        """
        bv = pdv.synthesize_batch({"prover": [self._prompt_fallback_record()],
                                   "adversary": []})
        self.assertEqual(bv.executed, 0)
        self.assertEqual(bv.total, 1)
        header = bv.report.splitlines()[0]
        self.assertIn("1 total", header)
        self.assertIn("0 with executed-probe evidence", header)
        self.assertIn("1 blocking", header)
        self.assertIn("0 malformed", header)

    # ── (2) a harness-level ENOENT is not a missing deliverable ──────────────

    def test_harness_error_with_enoent_stderr_still_blocks(self):
        """`python3: can't open file ...` is a broken harness, not an unwritten .ri."""
        rec = self._executed(
            "alpha script path cap", "HARNESS_ERROR",
            stderr="python3: can't open file 'scripts/prd-capability-check.py': "
                   "[Errno 2] No such file or directory")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertEqual(bv.blocking, ["alpha script path cap"])
        self.assertEqual(bv.fixture_absent, [],
                         "a harness-level ENOENT must not be reported as a missing "
                         "leaf deliverable")

    def test_harness_error_with_bare_errno_stderr_still_blocks(self):
        """The Rust-style spelling of the same failure is treated identically."""
        rec = self._executed("cwd cap", "HARNESS_ERROR",
                             stderr="failed to spawn probe: os error 2")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertEqual(bv.fixture_absent, [])

    def test_harness_error_enoent_is_classified_blocking(self):
        """Unit-level pin on the classify_record precedence order."""
        rec = self._executed("harness enoent cap", "HARNESS_ERROR",
                             stderr="No such file or directory")
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_BLOCKING)

    # ── (3) blast radius: neither downgrade is disabled for probe verdicts ───

    def test_fail_with_the_same_enoent_stderr_is_still_fixture_absent(self):
        """The carve-out still applies where its rationale applies — to a probe."""
        rec = dict(
            self._executed("missing deliverable cap", "FAIL",
                           stderr=reify_missing_fixture_stderr(_LEAF_FIXTURE)),
            command=["reify", "check", _LEAF_FIXTURE], fixture_present=False)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.fixture_absent, ["missing deliverable cap"])

    def test_evidence_free_fail_is_still_malformed(self):
        """The evidence gate is intact for the verdicts it was written for."""
        rec = dict(self._prompt_fallback_record("vacuous cap"), verdict="FAIL")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.malformed, ["vacuous cap"])

    def test_evidence_free_unprovable_is_still_malformed(self):
        """Same for UNPROVABLE — only HARNESS_ERROR is exempt."""
        rec = dict(self._prompt_fallback_record("vacuous cap"), verdict="UNPROVABLE")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.malformed, ["vacuous cap"])

    # ── (4) end to end through the CLI ───────────────────────────────────────

    def test_cli_exits_1_on_the_prompt_fallback_record_alone(self):
        """The exit code — what the Synthesize agent actually relays — is 1."""
        data = {"prover": [self._prompt_fallback_record()], "adversary": []}
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(data, f)
            tmp = f.name
        buf_out, buf_err = io.StringIO(), io.StringIO()
        try:
            with unittest.mock.patch("sys.stdout", buf_out), \
                 unittest.mock.patch("sys.stderr", buf_err):
                rc = pdv.main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 1, "a dead harness must not exit 0")
        payload = json.loads(buf_out.getvalue())
        self.assertTrue(payload["blocks"])
        self.assertEqual(payload["blocking"], ["leaf label (delta)"])
        self.assertEqual(payload["malformed"], [])
        self.assertEqual(payload["executed"], 0)


# ---------------------------------------------------------------------------
# task #7257 amendment: the report format cannot drift between sections
# ---------------------------------------------------------------------------

class TestEvidenceBlockIsShared(unittest.TestCase):
    """All three report sections render evidence through ONE builder.

    They drifted within one task: the fixture-absent branch silently omitted
    `stdout`, so a probe that wrote to stdout before failing to find its fixture
    lost that evidence — contrary to PRD §6 decision 4's "exact command +
    stdout/stderr + exit code".  A reader comparing sections must be comparing
    like with like.
    """

    _STDOUT = "partial output before the open failed"

    def _rec(self, capability: str, verdict: str, stderr: str,
             command=None, exit_code=1, fixture_present=True) -> dict:
        return {
            "capability": capability,
            "probe_kind": "ir",
            "verdict": verdict,
            "command": ["reify", "eval", "f.ri"] if command is None else command,
            "exit_code": exit_code,
            "stdout": self._STDOUT,
            "stderr": stderr,
            "fixture_present": fixture_present,
        }

    def test_fixture_absent_section_carries_stdout(self):
        """The drift that motivated the extraction."""
        bv = pdv.synthesize_batch({"prover": [self._rec(
            "fixture-absent cap", "FAIL",
            reify_missing_fixture_stderr("f.ri"), fixture_present=False)],
            "adversary": []})
        self.assertEqual(bv.fixture_absent, ["fixture-absent cap"])
        self.assertIn(f"  stdout:    {self._STDOUT}", bv.report)

    def test_malformed_section_carries_stdout(self):
        bv = pdv.synthesize_batch({"prover": [self._rec(
            "vacuous cap", "FAIL", "", command=[], exit_code=None)],
            "adversary": []})
        self.assertEqual(bv.malformed, ["vacuous cap"])
        self.assertIn(f"  stdout:    {self._STDOUT}", bv.report)

    def test_blocking_section_carries_stdout(self):
        bv = pdv.synthesize_batch({"prover": [self._rec(
            "real fail cap", "FAIL", "type mismatch")], "adversary": []})
        self.assertEqual(bv.blocking, ["real fail cap"])
        self.assertIn(f"  stdout:    {self._STDOUT}", bv.report)

    def test_all_three_sections_share_one_line_format(self):
        """Same record, three categories — the evidence lines must be identical."""
        common = dict(command=["reify", "eval", "f.ri"], exit_code=1)
        block = pdv._evidence_block("FAIL", "cap", "prover", "reify eval f.ri",
                                    1, self._STDOUT, "stderr text")
        for stderr, fixture_present, category in (
            ("type mismatch: expected axis", True, "blocking"),
            (reify_missing_fixture_stderr("f.ri"), False, "fixture-absent"),
        ):
            with self.subTest(category=category):
                bv = pdv.synthesize_batch({"prover": [
                    self._rec("cap", "FAIL", stderr, fixture_present=fixture_present,
                              **common)], "adversary": []})
                rendered = "\n".join(
                    ln for ln in bv.report.splitlines()
                    if ln.startswith(("[FAIL]", "  command:", "  exit_code:",
                                      "  stdout:", "  stderr:")))
                self.assertEqual(
                    rendered,
                    block.replace("stderr text", stderr),
                    f"the {category} section renders a different evidence block",
                )

    def test_empty_stdout_and_stderr_lines_are_omitted(self):
        """An evidence block never pads the report with contentless lines."""
        block = pdv._evidence_block("FAIL", "cap", "prover", "reify eval f.ri",
                                    1, "", "")
        self.assertNotIn("stdout:", block)
        self.assertNotIn("stderr:", block)
        self.assertEqual(block.splitlines()[0], "[FAIL] cap (role: prover)")


if __name__ == "__main__":
    unittest.main()
