#!/usr/bin/env python3
"""
test_prd_decompose_verify_synthesize.py — synthesize_batch(), evidence-gate, fixture-absent
and report-format tests for scripts/prd-decompose-verify.py.
"""

import io
import json
import os
import tempfile
import unittest
import unittest.mock
from typing import Any

from _pdv_test_support import _PDV_MJS, _load_pcc, _load_pdv, _reify_missing_fixture_stderr

pdv = _load_pdv()
pcc = _load_pcc()

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
# task #7257 step-05 (RED): fixture-absent ≠ falsification (ARM 1, item 4)
# ---------------------------------------------------------------------------

class TestFixtureAbsent(unittest.TestCase):
    """A probe that could not find its fixture has falsified nothing.

    During decompose, the .ri fixture a premise probes is very often the leaf's
    own deliverable — it does not exist yet, by construction.  Scoring the
    resulting ENOENT as a premise falsification reports a design defect where
    there is only a missing file, which is what the observed pi report did.

    GREEN in task #7257 step-06 (fixture_absent_evidence).
    """

    def _result(self, capability: str, verdict: str = "FAIL",
                stderr: str = "", exit_code: int = 1) -> dict:
        """An EXECUTED α result record (real command, real exit code)."""
        return {
            "capability": capability,
            "probe_kind": "ir",
            "verdict": verdict,
            "command": ["reify", "eval", _LEAF_FIXTURE],
            "exit_code": exit_code,
            "stdout": "",
            "stderr": stderr,
        }

    # ── (1) the verbatim stderr from the observed run ────────────────────────

    def test_observed_enoent_stderr_is_not_a_falsification(self):
        """reify's 'Error reading <fixture>: ... (os error 2)' → fixture-absent."""
        rec = self._result("fixture-absent cap",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertNotIn("fixture-absent cap", bv.blocking)
        self.assertFalse(bv.blocks)
        self.assertIn("fixture-absent cap", bv.fixture_absent)

    # ── (2) both signature forms, case-insensitively ─────────────────────────

    def test_bare_phrase_without_the_fixture_is_not_enough(self):
        """A bare 'No such file or directory' names no path, so it still blocks."""
        rec = self._result("bare-enoent cap", stderr="No such file or directory")
        self.assertFalse(pdv.fixture_absent_evidence(rec))
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_BLOCKING)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("bare-enoent cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    def test_signature_match_is_case_insensitive(self):
        """Lower-cased diagnostics match too — the signature is normalized."""
        rec = self._result(
            "lowercase-enoent cap",
            stderr=f"error reading {_LEAF_FIXTURE}: no such file or directory")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("lowercase-enoent cap", bv.fixture_absent)
        self.assertFalse(bv.blocks)

    # ── (3) the Rust io::Error rendering on its own ──────────────────────────

    def test_bare_os_error_2_signature(self):
        """Rust renders ENOENT as 'os error 2'; that alone classifies fixture-absent."""
        rec = self._result("os-error-2 cap",
                           stderr=f"failed to open input {_LEAF_FIXTURE}: os error 2")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("os-error-2 cap", bv.fixture_absent)
        self.assertFalse(bv.blocks)

    # ── (4)(5) the over-reach guards ─────────────────────────────────────────

    def test_unrelated_diagnostic_still_blocks(self):
        """A genuine falsification with an unrelated stderr is untouched."""
        rec = self._result("real fail cap", stderr="type mismatch: expected axis")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("real fail cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    def test_empty_stderr_still_blocks(self):
        """An executed FAIL with no stderr at all is still a falsification."""
        rec = self._result("silent fail cap", stderr="")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("silent fail cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    # ── (6) missing BINARY is not a missing fixture ──────────────────────────

    def test_binary_not_found_sentinel_still_blocks(self):
        """α's binary-not-found sentinel emits the SAME ENOENT text but must block.

        A missing `reify` binary is a real harness failure: nothing was probed
        and the batch cannot be trusted.  The fixture-absent carve-out must not
        swallow it just because the OS worded both errors the same way.
        """
        stderr = (f"{pcc._BINARY_NOT_FOUND_SENTINEL}: [Errno 2] "
                  "No such file or directory: 'target/release/reify'")
        rec = self._result("missing binary cap", verdict="HARNESS_ERROR",
                           stderr=stderr, exit_code=127)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks, "a missing binary must still block")
        self.assertIn("missing binary cap", bv.blocking)
        self.assertNotIn("missing binary cap", bv.fixture_absent)

    # ── (7) counters and report placement ────────────────────────────────────

    def test_fixture_absent_record_counts_as_executed(self):
        """The probe DID run — it just could not find its fixture."""
        rec = self._result("fixture-absent cap",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.executed, 1)
        self.assertEqual(bv.total, 1)

    def test_fixture_absent_is_named_in_its_own_report_section(self):
        """The capability stays visible, under a fixture-absent label."""
        rec = self._result("fixture-absent cap",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("fixture-absent cap", bv.report)
        self.assertIn("FIXTURE ABSENT", bv.report)

    def test_fixture_absent_is_not_counted_as_malformed(self):
        """The two categories are distinct: one ran without a target, one never ran."""
        rec = self._result("fixture-absent cap",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.malformed, [])

    def test_passing_record_with_enoent_stderr_is_untouched(self):
        """The carve-out applies to blocking verdicts only."""
        rec = self._result("passing cap", verdict="PASS",
                           stderr="No such file or directory", exit_code=0)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.fixture_absent, [])
        self.assertEqual(bv.blocking, [])
        self.assertFalse(bv.blocks)


# ---------------------------------------------------------------------------
# task #7257 step-17 (RED): the errno signature must be anchored, not a prefix
# ---------------------------------------------------------------------------

class TestFixtureAbsentErrnoAnchoring(unittest.TestCase):
    """`os error 2` as a bare substring swallows every errno that STARTS with 2.

    `_FIXTURE_ABSENT_SIGNATURES` matches with `any(sig in lowered ...)`, so the
    literal `"os error 2"` is a PREFIX test, not an errno test: it fires on
    ENOTDIR(20), EISDIR(21), EINVAL(22), EMFILE(24), ENOSPC(28) and every
    3-digit 2XX errno.  Each of those is a genuine, executed falsification that
    silently stops blocking — the fixture-absent carve-out (which exists so a
    not-yet-written deliverable is not scored as a design defect) becomes a
    hole that swallows real findings.  This is the exact failure mode the
    carve-out was added to avoid, inverted.

    GREEN in task #7257 step-18 (anchored `os error 2(?![0-9])`).

    The class carries BOTH halves deliberately: the hostile errnos that must
    start blocking again, AND the three genuine ENOENT spellings the suite
    already depends on, so the fix's blast radius is pinned from both sides.
    """

    #: (label, stderr) for errnos whose decimal rendering begins with "2" but
    #: which are NOT ENOENT.  Every one is a real failure that must block.
    HOSTILE_ERRNOS = (
        ("ENOTDIR", f"Error reading {_LEAF_FIXTURE}: Not a directory (os error 20)"),
        ("EISDIR", f"Error reading {_LEAF_FIXTURE}: Is a directory (os error 21)"),
        ("EINVAL", f"Error reading {_LEAF_FIXTURE}: Invalid argument (os error 22)"),
        ("EMFILE", f"Error reading {_LEAF_FIXTURE}: Too many open files (os error 24)"),
        ("ENOSPC",
         f"Error reading {_LEAF_FIXTURE}: No space left on device (os error 28)"),
    )
    ERRNO_212 = f"probe aborted reading {_LEAF_FIXTURE} (os error 212)"

    def _result(self, capability: str, verdict: str = "FAIL",
                stderr: str = "", exit_code: int = 1) -> dict:
        """An EXECUTED α result record (real command, real exit code).

        Same shape as TestFixtureAbsent._result — the evidence gate must be
        satisfied so classification reaches the fixture-absent branch at all.
        """
        return {
            "capability": capability,
            "probe_kind": "ir",
            "verdict": verdict,
            "command": ["reify", "eval", _LEAF_FIXTURE],
            "exit_code": exit_code,
            "stdout": "",
            "stderr": stderr,
        }

    # ── (1) every 2X errno is a real falsification and must block ────────────

    def test_two_x_errnos_still_block(self):
        """ENOTDIR/EISDIR/EINVAL/EMFILE/ENOSPC are falsifications, not absences."""
        for label, stderr in self.HOSTILE_ERRNOS:
            with self.subTest(errno=label):
                cap = f"{label} cap"
                rec = self._result(cap, stderr=stderr)
                bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
                self.assertTrue(
                    bv.blocks,
                    f"{label} ({stderr!r}) is a real failure and must block")
                self.assertIn(cap, bv.blocking)
                self.assertNotIn(cap, bv.fixture_absent)

    # ── (2) the 2XX range too ────────────────────────────────────────────────

    def test_three_digit_two_hundred_errno_still_blocks(self):
        """A 3-digit errno beginning with 2 must not be read as ENOENT either."""
        rec = self._result("errno-212 cap", stderr=self.ERRNO_212)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("errno-212 cap", bv.blocking)
        self.assertNotIn("errno-212 cap", bv.fixture_absent)

    # ── (3) unit-level sweep, so a regression localises to the matcher ───────

    def test_matcher_rejects_non_enoent_errnos_directly(self):
        """`fixture_absent_evidence` itself is False for every hostile errno.

        Asserted against the predicate rather than through `synthesize_batch`
        so a future regression points at the signature table, not at the
        classification pipeline downstream of it.
        """
        table = self.HOSTILE_ERRNOS + (("errno-212", self.ERRNO_212),)
        for label, stderr in table:
            with self.subTest(errno=label):
                rec = self._result(f"{label} cap", stderr=stderr)
                self.assertFalse(
                    pdv.fixture_absent_evidence(rec),
                    f"{stderr!r} is not a fixture-absent signature")

    # ── (4) REGRESSION PRESERVATION: genuine ENOENT still classifies ─────────

    def test_genuine_enoent_spellings_still_classify_fixture_absent(self):
        """The three spellings the suite already relies on must keep working.

        The anchor must permit a bare trailing `os error 2` at end-of-string
        (no closing paren) — requiring `)` would break the spelling asserted by
        TestFixtureAbsent.test_bare_os_error_2_signature.  The phrase-only
        spelling is the perror shape, `<program>: <path>: <strerror>`.
        """
        spellings = (
            ("paren", _reify_missing_fixture_stderr(_LEAF_FIXTURE)),
            ("bare-errno-eos", f"failed to open input {_LEAF_FIXTURE}: os error 2"),
            ("phrase-only", f"cat: {_LEAF_FIXTURE}: No such file or directory"),
        )
        for label, stderr in spellings:
            with self.subTest(spelling=label):
                cap = f"enoent-{label} cap"
                rec = self._result(cap, stderr=stderr)
                self.assertTrue(
                    pdv.fixture_absent_evidence(rec),
                    f"{stderr!r} is a genuine ENOENT spelling")
                bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
                self.assertIn(cap, bv.fixture_absent)
                self.assertFalse(bv.blocks)

    # ── (5) near-miss guard ──────────────────────────────────────────────────

    def test_no_such_device_blocks(self):
        """'No such device (os error 19)' matches NEITHER signature.

        Textually adjacent to ENOENT's phrase ("No such ...") and numerically
        adjacent to the 2X block, so it pins both edges at once.
        """
        rec = self._result(
            "enodev cap",
            stderr=f"Error reading {_LEAF_FIXTURE}: No such device (os error 19)")
        self.assertFalse(pdv.fixture_absent_evidence(rec))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("enodev cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])


# ---------------------------------------------------------------------------
# task #7304 (RED): the ENOENT must name the probe's own fixture
# ---------------------------------------------------------------------------

class TestFixtureAbsentNamesTheProbeTarget(unittest.TestCase):
    """Fixture-absent needs an ENOENT on a stderr line naming the probe's fixture.

    "Fixture absent" means the OS reported the probe's TARGET missing.  ENOENT
    text that is quoted, about another path, or on a line without the target
    says nothing about the target, so the FAIL it rides on must keep blocking.
    """

    def _result(self, capability: str, stderr: str, verdict: str = "FAIL",
                command=None) -> dict:
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["reify", "check", _LEAF_FIXTURE] if command is None else command,
            "exit_code": 1,
            "stdout": "",
            "stderr": stderr,
        }

    def _assert_blocks(self, rec: dict) -> None:
        cap = rec["capability"]
        self.assertFalse(pdv.fixture_absent_evidence(rec))
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_BLOCKING)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn(cap, bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    def test_genuine_fail_quoting_the_enoent_text_blocks(self):
        self._assert_blocks(self._result(
            "quoted enoent cap",
            'AssertionError: expected the import diagnostic to read '
            '"No such file or directory (os error 2)", got "module \'lib\' not found"'))

    def test_enoent_about_a_different_file_blocks(self):
        self._assert_blocks(self._result(
            "other file cap",
            _reify_missing_fixture_stderr("tests/prd-gate/fixtures/helper_lib.ri")))

    def test_bare_errno_without_a_path_blocks(self):
        self._assert_blocks(self._result("bare errno cap", "failed to spawn: os error 2"))

    def test_relay_mislabelled_harness_enoent_blocks(self):
        """A FAIL whose argv is α's own invocation: the ENOENT is about α's script."""
        self._assert_blocks(self._result(
            "relayed harness cap",
            "python3: can't open file 'scripts/prd-capability-check.py': "
            "[Errno 2] No such file or directory",
            command=["python3", "scripts/prd-capability-check.py",
                     "--json", "/tmp/ps.json"]))

    def test_fixture_and_enoent_on_different_lines_blocks(self):
        self._assert_blocks(self._result(
            "split lines cap",
            f"warning: in {_LEAF_FIXTURE}: unused parameter\n"
            + _reify_missing_fixture_stderr("tests/prd-gate/fixtures/other.ri")))

    def test_argv_without_an_argument_blocks(self):
        self._assert_blocks(self._result(
            "no argument cap", "bash: reify: No such file or directory",
            command=["reify"]))

    def test_enoent_line_after_warnings_is_fixture_absent(self):
        rec = self._result(
            "warned absent cap",
            "warning: W_X: something\n" + _reify_missing_fixture_stderr(_LEAF_FIXTURE))
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_FIXTURE_ABSENT)

    def test_longer_path_ending_in_the_fixture_name_blocks(self):
        """A relayed short fixture is a path token, not a suffix of any path."""
        for other in ("tests/prd-gate/fixtures/subleaf.ri", "/other/leaf.ri"):
            with self.subTest(other=other):
                self._assert_blocks(self._result(
                    f"suffix {other} cap", _reify_missing_fixture_stderr(other),
                    command=["reify", "check", "leaf.ri"]))

    def test_fixture_delimited_as_a_whole_token_is_fixture_absent(self):
        for label, stderr in (
            ("reify", _reify_missing_fixture_stderr("leaf.ri")),
            ("perror", "cat: leaf.ri: No such file or directory"),
            ("quoted", "can't open file 'leaf.ri': [Errno 2] No such file or directory"),
            ("backticked", "failed to read `leaf.ri` (os error 2)"),
        ):
            with self.subTest(spelling=label):
                rec = self._result(f"{label} cap", stderr,
                                   command=["reify", "check", "leaf.ri"])
                self.assertEqual(pdv.classify_record(rec), pdv.CAT_FIXTURE_ABSENT)

    def test_missing_grammar_fixture_still_blocks(self):
        """tree-sitter's missing-file report names neither the path nor ENOENT.

        Pins today's fail-closed outcome for grammar probes (#8122), whose
        fixture-absent case the reify-specific anchor cannot see.
        """
        self._assert_blocks(dict(
            self._result(
                "grammar absent cap",
                "Error: No files were found at or matched by the provided "
                "pathname/glob",
                command=["tree-sitter", "parse", "--quiet", _LEAF_FIXTURE]),
            probe_kind="grammar"))


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
        [Errno 2] No such file or directory`) matched the ENOENT phrase and was
        reported as "the fixture is the leaf's own deliverable" — the opposite
        diagnosis.

    The blast radius is pinned in both directions: an evidence-free FAIL is
    still MALFORMED, and a FAIL carrying the same ENOENT stderr is still
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
        with open(_PDV_MJS, encoding="utf-8") as fh:
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
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE)),
            command=["reify", "check", _LEAF_FIXTURE])
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
             command=None, exit_code=1) -> dict:
        return {
            "capability": capability,
            "probe_kind": "ir",
            "verdict": verdict,
            "command": ["reify", "eval", "f.ri"] if command is None else command,
            "exit_code": exit_code,
            "stdout": self._STDOUT,
            "stderr": stderr,
        }

    def test_fixture_absent_section_carries_stdout(self):
        """The drift that motivated the extraction."""
        bv = pdv.synthesize_batch({"prover": [self._rec(
            "fixture-absent cap", "FAIL",
            _reify_missing_fixture_stderr("f.ri"))], "adversary": []})
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
        for stderr, category in (
            ("type mismatch: expected axis", "blocking"),
            (_reify_missing_fixture_stderr("f.ri"), "fixture-absent"),
        ):
            with self.subTest(category=category):
                bv = pdv.synthesize_batch({"prover": [
                    self._rec("cap", "FAIL", stderr, **common)], "adversary": []})
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
