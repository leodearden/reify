#!/usr/bin/env python3
"""
test_prd_decompose_verify.py — main() CLI, boundary e2e, user-observable-signal and
binding-row CLI tests for scripts/prd-decompose-verify.py.  Sibling partitions _bind,
_synthesize and _mjs run with this one in a single unittest invocation from
tests/infra/test_prd_decompose_verify.sh.
"""

import glob
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
from dataclasses import dataclass

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _pdv_test_support import REPO_ROOT, load_pcc, load_pdv, reify_missing_fixture_stderr

pdv = load_pdv()
pcc = load_pcc()

# ---------------------------------------------------------------------------
# Repo-root helpers for skip-guards
# ---------------------------------------------------------------------------

_REIFY_RELEASE = os.path.join(REPO_ROOT, "target", "release", "reify")
_REIFY_DEBUG = os.path.join(REPO_ROOT, "target", "debug", "reify")
_REIFY_BUILT = os.path.isfile(_REIFY_RELEASE) or os.path.isfile(_REIFY_DEBUG)

_LEAF_FALSE = os.path.join(REPO_ROOT, "tests", "prd-gate", "leaf-false-premise.json")
_LEAF_TRUE = os.path.join(REPO_ROOT, "tests", "prd-gate", "leaf-true-premise.json")


# ---------------------------------------------------------------------------
# The α ↔ pdv boundary, driven through both CLIs over temp files
# ---------------------------------------------------------------------------
# These are the bytes the D3 Prover relays, so a test of the boundary reads α's
# real --json record rather than a hand-built projection of it.

_PDV_SCRIPT = os.path.join(REPO_ROOT, "scripts", "prd-decompose-verify.py")
_ALPHA_SCRIPT = os.path.join(REPO_ROOT, "scripts", "prd-capability-check.py")

# α's exit codes for a probe set it evaluated (64 is a usage error: no records).
_ALPHA_EVALUATED_RCS = (0, 1, 2, 70)


def _run_script(script, *argv, env=None):
    return subprocess.run(
        [sys.executable, script, *argv], capture_output=True, text=True, env=env
    )


def _write_json(dirpath, name, obj):
    path = os.path.join(dirpath, name)
    with open(path, "w") as fh:
        json.dump(obj, fh)
    return path


def _alpha_json(probe_set, tmpdir, env=None):
    """Run α --json over `probe_set`; return (rc, records)."""
    proc = _run_script(
        _ALPHA_SCRIPT, "--json", _write_json(tmpdir, "probes.json", probe_set), env=env
    )
    if proc.returncode not in _ALPHA_EVALUATED_RCS:
        raise AssertionError(
            f"α --json exited {proc.returncode}, evaluating nothing: {proc.stderr}"
        )
    return proc.returncode, json.loads(proc.stdout)["results"]


@dataclass(frozen=True)
class _ChainRun:
    """What pdv bind → α --json → pdv synthesize produced."""
    alpha_rc: int
    records: list
    synthesize_rc: int
    verdict: dict


def _bind_probe_synthesize(premises, tmpdir, env=None):
    """Chain the three CLIs over `premises`, every hop through a temp file."""
    bind = _run_script(
        _PDV_SCRIPT, "bind", _write_json(tmpdir, "premises.json", {"premises": premises}),
        env=env,
    )
    if bind.returncode != 0:
        raise AssertionError(f"pdv bind exited {bind.returncode}: {bind.stderr}")
    alpha_rc, records = _alpha_json(json.loads(bind.stdout), tmpdir, env=env)
    synthesize = _run_script(
        _PDV_SCRIPT, "synthesize",
        _write_json(tmpdir, "results.json", {"prover": records, "adversary": []}),
        env=env,
    )
    if synthesize.returncode not in (0, 1):
        raise AssertionError(
            f"pdv synthesize exited {synthesize.returncode}: {synthesize.stderr}"
        )
    return _ChainRun(alpha_rc, records, synthesize.returncode,
                     json.loads(synthesize.stdout))


# ---------------------------------------------------------------------------
# step-07 (RED): CLI main(argv) integration
# ---------------------------------------------------------------------------

class TestMainCLI(unittest.TestCase):
    """Tests for main(argv) subcommands (bind / synthesize).

    These tests FAIL until step-08 implements main() properly.
    """

    def _run_main(self, argv):
        """Run pdv.main(argv) capturing stdout/stderr; returns (exit_code, stdout, stderr)."""
        buf_out = io.StringIO()
        buf_err = io.StringIO()
        with unittest.mock.patch("sys.stdout", buf_out), \
             unittest.mock.patch("sys.stderr", buf_err):
            rc = pdv.main(argv)
        return rc, buf_out.getvalue(), buf_err.getvalue()

    # ── --help ────────────────────────────────────────────────────────────────

    def test_help_exits_0(self):
        """main(['--help']) → 0."""
        rc, _, _ = self._run_main(["--help"])
        self.assertEqual(rc, 0)

    # ── no args / usage errors → 64 ──────────────────────────────────────────

    def test_no_args_exits_64(self):
        """main([]) → 64 (EX_USAGE)."""
        rc, _, _ = self._run_main([])
        self.assertEqual(rc, 64)

    def test_bind_missing_file_exits_64(self):
        """main(['bind', '/nonexistent']) → 64 (IO error)."""
        rc, _, _ = self._run_main(["bind", "/nonexistent-premises-xyz.json"])
        self.assertEqual(rc, 64)

    def test_synthesize_missing_file_exits_64(self):
        """main(['synthesize', '/nonexistent']) → 64 (IO error)."""
        rc, _, _ = self._run_main(["synthesize", "/nonexistent-results-xyz.json"])
        self.assertEqual(rc, 64)

    def test_bind_bad_json_exits_64(self):
        """main(['bind', <invalid-json-file>]) → 64."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write("not json at all")
            tmp = f.name
        try:
            rc, _, _ = self._run_main(["bind", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 64)

    # ── bind subcommand ───────────────────────────────────────────────────────

    def test_bind_writes_probe_set_json_to_stdout(self):
        """main(['bind', <premises.json>]) writes valid α probe-set JSON to stdout."""
        premises_data = {
            "premises": [
                {
                    "text": "revolute rejects non-axis",
                    "assertion_kind": "rejection",
                    "fixture": "tests/prd-gate/fixtures/revolute_silent_accept.ri",
                    "match": {"exit_code": 1},
                    "capability": "arg-vs-param rejection",
                }
            ]
        }
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(premises_data, f)
            tmp = f.name
        try:
            rc, out, _ = self._run_main(["bind", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0, f"bind must exit 0, got {rc}")
        # Output must be parseable JSON
        try:
            obj = json.loads(out)
        except json.JSONDecodeError as e:
            self.fail(f"bind stdout is not valid JSON: {e}\nGot: {out!r}")
        # Must be accepted by α's load_probe_set
        pcc.load_probe_set(out)  # must not raise

    def test_bind_exits_0(self):
        """main(['bind', <valid-premises.json>]) exits 0."""
        premises_data = {
            "premises": [
                {
                    "text": "test premise",
                    "assertion_kind": "parses",
                    "fixture": "tests/prd-gate/fixtures/ir_clean_eval.ri",
                    "match": {},
                    "capability": "test",
                }
            ]
        }
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(premises_data, f)
            tmp = f.name
        try:
            rc, _, _ = self._run_main(["bind", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0)

    # ── synthesize subcommand ─────────────────────────────────────────────────

    def _make_results_file(self, prover=None, adversary=None):
        """Write a results JSON file and return its path."""
        data = {
            "prover": prover or [],
            "adversary": adversary or [],
        }
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(data, f)
            return f.name

    def _result_record(self, capability: str, verdict: str, exit_code=0,
                       stdout="", stderr="") -> dict:
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["reify", "check", "/fixture.ri"],
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }

    def test_synthesize_all_pass_exits_0(self):
        """synthesize with all-PASS results exits 0."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap", "PASS")],
            adversary=[],
        )
        try:
            rc, _, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0)

    def test_synthesize_with_fail_exits_1(self):
        """synthesize with a FAIL result exits 1."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap-FAIL", "FAIL",
                                        exit_code=0, stdout="All constraints satisfied.")],
        )
        try:
            rc, _, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 1)

    def test_synthesize_emits_batch_verdict_json(self):
        """synthesize emits a BatchVerdict JSON to stdout."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap", "PASS")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        try:
            obj = json.loads(out)
        except json.JSONDecodeError as e:
            self.fail(f"synthesize stdout is not valid JSON: {e}\nGot: {out!r}")
        self.assertIn("blocks", obj)
        self.assertIn("blocking", obj)

    def test_synthesize_blocking_verdict_json_has_blocks_true(self):
        """synthesize with FAIL result: 'blocks' field is true in JSON output."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap-F", "FAIL", exit_code=0)],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        obj = json.loads(out)
        self.assertTrue(obj["blocks"])

    # ── task #7257 step-07 (RED): the evidence gate on the CLI surface ───────

    def _evidence_free_record(self, capability: str, verdict: str = "FAIL") -> dict:
        """A blocking verdict carrying no executed-probe evidence."""
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": [],
            "exit_code": None,
            "stdout": "",
            "stderr": "",
        }

    def _fixture_absent_record(self, capability: str) -> dict:
        """An executed FAIL whose probe could not find its target file."""
        return self._result_record(
            capability, "FAIL", exit_code=1,
            stderr=reify_missing_fixture_stderr("/fixture.ri"),
        )

    def test_synthesize_only_evidence_free_fails_exits_0(self):
        """A file of nothing but unexecuted promises does not block the batch."""
        tmp = self._make_results_file(
            prover=[self._evidence_free_record("vacuous-1"),
                    self._evidence_free_record("vacuous-2", "UNPROVABLE")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0, f"evidence-free records must not block; stdout={out!r}")
        obj = json.loads(out)
        self.assertFalse(obj["blocks"])
        self.assertEqual(sorted(obj["malformed"]), ["vacuous-1", "vacuous-2"])

    def test_synthesize_one_executed_fail_among_vacuous_exits_1_with_one_blocker(self):
        """The real finding still blocks, and it is the ONLY thing listed."""
        tmp = self._make_results_file(
            prover=[self._evidence_free_record("vacuous-1"),
                    self._result_record("REAL fail", "FAIL", exit_code=1,
                                        stderr="type mismatch: expected axis"),
                    self._evidence_free_record("vacuous-2")],
            adversary=[self._evidence_free_record("vacuous-3")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 1)
        obj = json.loads(out)
        self.assertEqual(obj["blocking"], ["REAL fail"])
        self.assertEqual(len(obj["blocking"]), 1)

    def test_synthesize_json_carries_all_seven_keys_with_right_types(self):
        """The emitted JSON is the full BatchVerdict, not just the old three keys."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap", "PASS")],
        )
        try:
            _, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        obj = json.loads(out)
        for key in ("blocks", "blocking", "report", "malformed",
                    "fixture_absent", "executed", "total"):
            self.assertIn(key, obj, f"synthesize JSON is missing {key!r}")
        self.assertIsInstance(obj["blocks"], bool)
        self.assertIsInstance(obj["blocking"], list)
        self.assertIsInstance(obj["report"], str)
        self.assertIsInstance(obj["malformed"], list)
        self.assertIsInstance(obj["fixture_absent"], list)
        self.assertIsInstance(obj["executed"], int)
        self.assertIsInstance(obj["total"], int)

    def test_synthesize_only_fixture_absent_exits_0_and_names_it(self):
        """A missing fixture is a deliverable signal, not a batch-blocking failure."""
        tmp = self._make_results_file(
            prover=[self._fixture_absent_record("fixture-absent cap")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0, f"fixture-absent must not block; stdout={out!r}")
        obj = json.loads(out)
        self.assertFalse(obj["blocks"])
        self.assertEqual(obj["fixture_absent"], ["fixture-absent cap"])

    def test_synthesize_all_pass_reports_a_clean_basis(self):
        """Happy-path regression guard: executed == total, nothing set aside."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap-A", "PASS"),
                    self._result_record("cap-B", "PASS")],
            adversary=[self._result_record("cap-C", "PASS")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0)
        obj = json.loads(out)
        self.assertEqual(obj["total"], 3)
        self.assertEqual(obj["executed"], 3)
        self.assertEqual(obj["malformed"], [])
        self.assertEqual(obj["fixture_absent"], [])


# ---------------------------------------------------------------------------
# step-11 (RED): consumer-side boundary e2e (PRD §10), skip-guarded on reify
# ---------------------------------------------------------------------------

class TestBoundaryE2e(unittest.TestCase):
    """Consumer-side boundary tests: false-premise leaf BLOCKS, true-premise leaf PASSES.

    PRD §10 boundary contract, realized at the deterministic-harness layer.
    Skip-guarded on a built reify binary (both the false and true leaf need real eval).
    These tests FAIL until step-12 finalizes the fixtures and synthesis glue.
    """

    def _load_premises_from_leaf(self, leaf_path: str):
        """Read a leaf fixture JSON and return a list of Premise objects."""
        with open(leaf_path) as fh:
            data = json.load(fh)
        premises = []
        for rec in data["premises"]:
            premises.append(pdv.Premise(
                text=rec["text"],
                assertion_kind=rec["assertion_kind"],
                fixture=rec["fixture"],
                match=rec.get("match", {}),
                capability=rec.get("capability"),
            ))
        return premises

    def _run_probes_with_alpha(self, probe_set_dict: dict) -> list:
        """α's own --json records for probe_set_dict, through its CLI."""
        with tempfile.TemporaryDirectory(prefix="pdv_e2e_alpha_") as tmpdir:
            _, records = _alpha_json(probe_set_dict, tmpdir)
        return records

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_false_premise_leaf_blocks(self):
        """leaf-false-premise.json → bind → α evaluate → synthesize → blocks==True.

        PRD §10: the §3 4575 silent-accept leaf MUST block.
        The rejection probe expects exit_code:1 (rejection fires), but real reify
        exits 0 ('All constraints satisfied.') → ABSENT → expected present → FAIL.
        """
        premises = self._load_premises_from_leaf(_LEAF_FALSE)
        self.assertGreater(len(premises), 0, "leaf-false-premise.json must have premises")

        probe_set = pdv.bind_premises(premises)
        results = self._run_probes_with_alpha(probe_set)

        bv = pdv.synthesize_batch({"prover": results, "adversary": []})

        self.assertTrue(bv.blocks,
                        "leaf-false-premise.json must BLOCK (4575 silent-accept leaf must fail)")

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_false_premise_report_contains_4575_evidence(self):
        """Blocking report contains captured 4575 evidence (exit_code 0, 'All constraints satisfied.')."""
        premises = self._load_premises_from_leaf(_LEAF_FALSE)
        probe_set = pdv.bind_premises(premises)
        results = self._run_probes_with_alpha(probe_set)
        bv = pdv.synthesize_batch({"prover": results, "adversary": []})

        self.assertTrue(bv.blocks, "must block to have evidence in report")
        # The real reify exits 0 with 'All constraints satisfied.' for revolute_silent_accept.ri
        self.assertIn("All constraints satisfied.", bv.report,
                      "report must capture the 4575 evidence: 'All constraints satisfied.'")

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_true_premise_leaf_passes(self):
        """leaf-true-premise.json → bind → α evaluate → synthesize → blocks==False."""
        premises = self._load_premises_from_leaf(_LEAF_TRUE)
        self.assertGreater(len(premises), 0, "leaf-true-premise.json must have premises")

        probe_set = pdv.bind_premises(premises)
        results = self._run_probes_with_alpha(probe_set)

        bv = pdv.synthesize_batch({"prover": results, "adversary": []})

        self.assertFalse(bv.blocks,
                         f"leaf-true-premise.json must PASS; blocking: {bv.blocking}; "
                         f"report: {bv.report}")

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_real_alpha_missing_fixture_is_fixture_absent(self):
        """Real α + reify on a never-written fixture is fixture-absent, not blocking.

        Pins the fixture anchor against what α actually records, so #7257's
        false block cannot quietly return.
        """
        with tempfile.TemporaryDirectory(prefix="pdv_e2e_absent_") as tmpdir:
            fixture = os.path.join(tmpdir, "never_written.ri")
            results = self._run_probes_with_alpha({"probes": [
                {"capability": "ir absent", "probe_kind": "ir", "fixture": fixture,
                 "expected": {"observation": "present",
                              "match": {"stderr_contains": "E_NEVER"}}},
                {"capability": "check absent", "probe_kind": "check",
                 "fixture": fixture,
                 "expected": {"observation": "present", "match": {"exit_code": 0}}},
            ]})
        bv = pdv.synthesize_batch({"prover": results, "adversary": []})
        self.assertEqual(sorted(bv.fixture_absent), ["check absent", "ir absent"],
                         bv.report)
        self.assertFalse(bv.blocks, bv.report)


class TestFixtureAbsentBoundary(unittest.TestCase):
    """α measures fixture presence and pdv sets the absence aside (PRD §6 B1, §3 M1).

    Driven through the three CLIs, so pdv reads exactly the record α emits.
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="pdv_fixture_absent_")
        self.addCleanup(shutil.rmtree, self._tmpdir, ignore_errors=True)
        self._fixture = os.path.join(self._tmpdir, "s22_does_not_exist.ri")
        self.assertFalse(os.path.exists(self._fixture), "precondition: never written")

    def _rejection(self, capability):
        return {"text": capability, "capability": capability,
                "assertion_kind": "rejection", "fixture": self._fixture,
                "match": {"exit_code": 1}}

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_b1_rejection_on_a_missing_fixture_is_fixture_absent(self):
        run = _bind_probe_synthesize([self._rejection("B1 rejection")], self._tmpdir)
        [record] = run.records
        self.assertEqual(record["verdict"], pcc.UNPROVABLE, record)
        self.assertIs(record["fixture_present"], False)
        self.assertEqual(record["fixture"], self._fixture)
        self.assertEqual(run.alpha_rc, 2)
        self.assertEqual(run.verdict["fixture_absent"], ["B1 rejection"])
        self.assertIs(run.verdict["blocks"], False)
        self.assertEqual(run.verdict["executed"], 1)

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_m1_reproduction_is_fixture_absent_not_verified(self):
        """The two-premise set that read VERIFIED at PRD §3 M1."""
        premises = [
            self._rejection("M1 rejection"),
            {"text": "M1 resolves", "capability": "M1 resolves",
             "assertion_kind": "resolves", "fixture": self._fixture, "match": {}},
        ]
        run = _bind_probe_synthesize(premises, self._tmpdir)
        self.assertEqual(
            sorted(run.verdict["fixture_absent"]), ["M1 rejection", "M1 resolves"],
            run.verdict["report"],
        )
        self.assertIs(run.verdict["blocks"], False)
        self.assertEqual(run.verdict["executed"], 2)


# ---------------------------------------------------------------------------
# task #7257 step-15 (RED): the user-observable signal, end to end
# ---------------------------------------------------------------------------

class TestUserObservableSignal(unittest.TestCase):
    """One synthesize call must make all four ARM 1 sub-signals legible at once.

    This is the report a human actually reads.  The observed pi report scored
    five capabilities as blocking when exactly one had been probed, rendered a
    string command character-by-character, and stated nowhere how many of its
    records carried evidence.  A reader could not tell a falsification from an
    unexecuted promise, and had no count to check the verdict against.

    GREEN in task #7257 step-16 (the counts header).
    """

    _STRING_CMD = "target/release/reify eval f.ri"
    _EXPLODED = "t a r g e t"

    def _mixed_batch(self):
        """One executed FAIL + three evidence-free + one fixture-absent + one string-command."""
        return {
            "prover": [
                # (1) the ONE genuine, evidence-backed falsification
                {"capability": "REAL executed fail", "probe_kind": "check",
                 "verdict": "FAIL",
                 "command": ["reify", "check", "tests/prd-gate/fixtures/x.ri"],
                 "exit_code": 0, "stdout": "All constraints satisfied.", "stderr": ""},
                # (2) three unexecuted promises
                {"capability": "vacuous-1", "probe_kind": "check", "verdict": "FAIL",
                 "command": [], "exit_code": None, "stdout": "", "stderr": ""},
                {"capability": "vacuous-2", "probe_kind": "ir", "verdict": "UNPROVABLE",
                 "command": [], "stdout": "", "stderr": ""},
                {"capability": "vacuous-3", "probe_kind": "check", "verdict": "FAIL",
                 "exit_code": None, "stdout": "", "stderr": ""},
                # (3) a probe that ran but could not find its fixture
                {"capability": "fixture-absent cap", "probe_kind": "ir", "verdict": "FAIL",
                 "command": ["reify", "eval", "tests/prd-gate/fixtures/not-yet.ri"],
                 "exit_code": 1, "stdout": "",
                 "stderr": reify_missing_fixture_stderr(
                     "tests/prd-gate/fixtures/not-yet.ri")},
            ],
            "adversary": [
                # (4) an evidence-backed falsification whose command is a STRING
                {"capability": "string-command cap", "probe_kind": "ir", "verdict": "FAIL",
                 "command": self._STRING_CMD, "exit_code": 1, "stdout": "",
                 "stderr": "assertion did not hold"},
            ],
        }

    def test_only_evidence_backed_records_block(self):
        """blocks is true, and `blocking` is EXACTLY the evidence-backed set.

        The string-command record blocks too, and correctly so — a string
        command normalizes to one token, so the record does carry executed-probe
        evidence.  What it must NOT do is render exploded (asserted below).
        The three vacuous records and the fixture-absent one are what disappear
        from `blocking`, which is the signal: 6 records in, 2 real findings out,
        not 6 undifferentiated blockers.
        """
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertTrue(bv.blocks)
        self.assertEqual(sorted(bv.blocking), ["REAL executed fail", "string-command cap"])

    def test_the_three_unexecuted_promises_are_listed_as_malformed(self):
        """Each vacuous record is still named — just under the right heading."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertEqual(sorted(bv.malformed), ["vacuous-1", "vacuous-2", "vacuous-3"])

    def test_the_fixture_absent_record_is_listed_separately(self):
        """A missing deliverable is its own category, not a falsification."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertEqual(bv.fixture_absent, ["fixture-absent cap"])

    def test_the_string_command_renders_verbatim(self):
        """Captured evidence stays re-runnable."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertIn(self._STRING_CMD, bv.report)
        self.assertNotIn(self._EXPLODED, bv.report)

    def test_report_opens_with_a_machine_readable_counts_header(self):
        """The report states its own basis on line 1, before any evidence block.

        Without this a reader has to count report sections by hand to learn how
        much of the batch was actually probed — which is exactly the arithmetic
        nobody did on the pi report.
        """
        bv = pdv.synthesize_batch(self._mixed_batch())
        header = bv.report.splitlines()[0]
        self.assertIn("records:", header, f"report header missing; got {header!r}")
        for token in ("6 total", "3 with executed-probe evidence", "2 blocking",
                      "3 malformed", "1 fixture-absent"):
            self.assertIn(token, header,
                          f"counts header must state {token!r}; got {header!r}")

    def test_counts_header_is_emitted_even_when_nothing_blocks(self):
        """An all-clear report still states the basis of its all-clear."""
        bv = pdv.synthesize_batch({
            "prover": [{"capability": "cap", "probe_kind": "check", "verdict": "PASS",
                        "command": ["reify", "check", "f.ri"], "exit_code": 0,
                        "stdout": "", "stderr": ""}],
            "adversary": [],
        })
        header = bv.report.splitlines()[0]
        self.assertIn("records:", header, f"report header missing; got {header!r}")
        self.assertIn("1 total", header)
        self.assertIn("0 blocking", header)

    def test_counts_header_agrees_with_the_structured_fields(self):
        """The header is derived from the same counters, never hand-maintained."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        header = bv.report.splitlines()[0]
        self.assertIn(f"{bv.total} total", header)
        self.assertIn(f"{bv.executed} with executed-probe evidence", header)
        self.assertIn(f"{len(bv.blocking)} blocking", header)
        self.assertIn(f"{len(bv.malformed)} malformed", header)
        self.assertIn(f"{len(bv.fixture_absent)} fixture-absent", header)



# ---------------------------------------------------------------------------
# #8644 (S22 δ, B6/B7): the binding row's required match, through both real CLIs
# ---------------------------------------------------------------------------

_PDV_SCRIPT = os.path.join(REPO_ROOT, "scripts", "prd-decompose-verify.py")
_ALPHA_SCRIPT = os.path.join(REPO_ROOT, "scripts", "prd-capability-check.py")


class TestBindingRowCli(unittest.TestCase):
    """`bind` writes a row's required match, and α then executes it.

    Both scripts run as real processes.  α's reify is a /bin/sh stub named by
    REIFY_BIN, and the fixture is an existing file given by absolute path, so
    the verdict depends on the stub's exit status alone.
    """

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="pdv_binding_row_")
        self.addCleanup(self._tmp.cleanup)
        self.tmp = self._tmp.name
        self.fixture = os.path.join(self.tmp, "f.ri")
        with open(self.fixture, "w") as fh:
            fh.write("// existing fixture; the stub reify never reads it\n")

    def _write_json(self, name, obj):
        path = os.path.join(self.tmp, name)
        with open(path, "w") as fh:
            json.dump(obj, fh)
        return path

    def _stub_reify(self, status):
        path = os.path.join(self.tmp, f"reify-exit-{status}")
        with open(path, "w") as fh:
            fh.write(f"#!/bin/sh\necho 'stub reify: exiting {status}' >&2\nexit {status}\n")
        os.chmod(path, 0o755)
        return path

    def _bind(self, premises_path):
        return subprocess.run(
            [sys.executable, _PDV_SCRIPT, "bind", premises_path],
            capture_output=True, text=True,
        )

    def _resolves_premises(self, match):
        return self._write_json("premises.json", {"premises": [{
            "text": "f.ri resolves",
            "assertion_kind": "resolves",
            "fixture": self.fixture,
            "match": match,
        }]})

    def test_b6_a_resolves_premise_with_no_match_binds_an_executed_exit_code_0(self):
        bind = self._bind(self._resolves_premises({}))
        self.assertEqual(bind.returncode, 0, bind.stderr)
        probe_set = json.loads(bind.stdout)
        self.assertEqual(probe_set["probes"][0]["expected"]["match"], {"exit_code": 0})

        probe_set_path = os.path.join(self.tmp, "probe-set.json")
        with open(probe_set_path, "w") as fh:
            fh.write(bind.stdout)
        for status, verdict, alpha_rc in ((1, "FAIL", 1), (0, "PASS", 0)):
            with self.subTest(reify_exit=status):
                alpha = subprocess.run(
                    [sys.executable, _ALPHA_SCRIPT, "--json", probe_set_path],
                    capture_output=True, text=True,
                    env=dict(os.environ, REIFY_BIN=self._stub_reify(status)),
                )
                self.assertEqual(alpha.returncode, alpha_rc, alpha.stderr)
                results = json.loads(alpha.stdout)["results"]
                self.assertEqual(results[0]["verdict"], verdict, results)

    def test_b7_a_resolves_premise_that_demands_exit_code_1_is_refused(self):
        bind = self._bind(self._resolves_premises({"exit_code": 1}))
        self.assertEqual(bind.returncode, 64, bind.stdout)
        for needle in ("resolves", "exit_code"):
            self.assertIn(needle, bind.stderr)
        self.assertRegex(bind.stderr, r"\b0\b")
        self.assertRegex(bind.stderr, r"\b1\b")
        self.assertEqual(bind.stdout.strip(), "")

    def test_every_committed_premises_file_binds(self):
        premise_files = []
        for path in sorted(glob.glob(os.path.join(REPO_ROOT, "tests", "prd-gate", "*.json"))):
            with open(path) as fh:
                doc = json.load(fh)
            if isinstance(doc, dict) and "premises" in doc:
                premise_files.append(path)
        self.assertTrue(premise_files, "no committed premises file under tests/prd-gate")
        for path in premise_files:
            with self.subTest(premises=os.path.basename(path)):
                bind = self._bind(path)
                self.assertEqual(bind.returncode, 0, bind.stderr)


if __name__ == "__main__":
    unittest.main()
