#!/usr/bin/env python3
"""
test_reify_audit_pdoccover.py — the PDOCCOVER hard gate's hermetic behaviour
matrix, driven through the real reify-audit binary over staged fixture repos.

WHAT RUNS THIS: tests/infra/test_reify_audit_pdoccover.sh, the member
run_all.sh discovers. It provisions the binary and exports three variables,
each REQUIRED here (a missing one fails the run rather than skipping it):

  REIFY_AUDIT_BIN                   the reify-audit binary under test.
  REIFY_GIT_ENV_SCRUB_VARS          names removed from every fixture `git`
                                    spawn; scripts/lib_git_env_scrub.sh is the
                                    one list.
  REIFY_PDOCCOVER_BIN_MAY_BE_STALE  "1" when the wrapper could not vouch for
                                    the binary's freshness.

Two classes. StalenessStable holds for any binary that has PDOCCOVER at all.
LedgerGrammar exercises the `<chunk path>:<name>` row kind and the uniform
stale-row rule, which a binary predating them cannot know, so it is skipped
when the binary may be stale.

Every case asserts the EXACT exit code (reify-audit exits with its High count)
AND the finding's `<category>: <name> ` summary prefix on stderr: an exit code
alone is satisfied by any failure. Chunk prose in omission cases avoids
`ident(` shapes, since the fixture's only oracle source is units.rs.
"""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


def _required_env(name):
    value = os.environ.get(name)
    if value is None:
        sys.exit(f"{Path(__file__).name}: {name} is not set — run this through "
                 "tests/infra/test_reify_audit_pdoccover.sh")
    return value


AUDIT_BIN = _required_env("REIFY_AUDIT_BIN")
SCRUB_VARS = _required_env("REIFY_GIT_ENV_SCRUB_VARS").split()
BIN_MAY_BE_STALE = _required_env("REIFY_PDOCCOVER_BIN_MAY_BE_STALE") == "1"

GIT_ENV = {k: v for k, v in os.environ.items() if k not in SCRUB_VARS}

UNITS = "crates/reify-compiler/src/units.rs"
CHUNK = "crates/reify-mcp/src/tools/chunks/stdlib.md"
BASELINE = "crates/reify-audit/pdoccover-baseline.txt"
GHOST_ROW = f"{CHUNK}:ghost_op"

# Exit codes that mean the run itself failed rather than reported: 101 is a
# Rust panic, 125 an IO/argument misconfiguration, a negative code a signal.
TRANSIENT = {101, 125}


def registry(*names):
    entries = "".join(f'    "{name}",\n' for name in names)
    return f"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n{entries}];\n"


DOCUMENTED_CHUNK = "# Stdlib\n\n- `documented_op` — documented.\n"
GHOST_CHUNK = DOCUMENTED_CHUNK + "- `ghost_op(x)` — ahead of the implementation.\n"


class GateCase(unittest.TestCase):
    """One staged fixture repo per test, plus the aux inputs every run needs."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        work = Path(tmp.name)
        self.repo = work / "repo"
        self.repo.mkdir()
        self.tasks_file = work / "tasks.json"
        self.tasks_file.write_text("[]")
        self.runs_db = work / "runs.db"
        self.runs_db.touch()

    def stage(self, files):
        for rel, content in files.items():
            path = self.repo / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        self.git("init", "-q")
        self.git("add", "-A")

    def git(self, *args):
        result = subprocess.run(
            ["git", "-C", str(self.repo), *args],
            capture_output=True, text=True, env=GIT_ENV,
        )
        self.assertEqual(result.returncode, 0, f"git {args}: {result.stderr}")

    def audit(self):
        for _ in range(3):
            result = subprocess.run(
                [AUDIT_BIN, "--pattern", "PDOCCOVER",
                 "--project-root", str(self.repo),
                 "--tasks-file", str(self.tasks_file),
                 "--runs-db", str(self.runs_db),
                 "--no-jcodemunch"],
                capture_output=True, text=True,
            )
            if result.returncode >= 0 and result.returncode not in TRANSIENT:
                break
        return result

    def assert_gate(self, exit_code, summary_prefix=None):
        result = self.audit()
        self.assertEqual(
            result.returncode, exit_code,
            f"reify-audit stderr tail:\n{result.stderr[-3000:]}",
        )
        if summary_prefix is not None:
            self.assertIn(summary_prefix, result.stderr)


class StalenessStable(GateCase):

    def test_an_undocumented_registry_name_reds_the_gate(self):
        self.stage({UNITS: registry("lonely_op"), CHUNK: DOCUMENTED_CHUNK})
        self.assert_gate(1, "undocumented-name: lonely_op ")

    def test_a_bare_row_absorbs_its_omission(self):
        self.stage({
            UNITS: registry("lonely_op"),
            CHUNK: DOCUMENTED_CHUNK,
            BASELINE: "lonely_op\n",
        })
        self.assert_gate(0)

    def test_a_bare_row_for_a_documented_name_is_stale(self):
        self.stage({
            UNITS: registry("documented_op"),
            CHUNK: DOCUMENTED_CHUNK,
            BASELINE: "documented_op\n",
        })
        self.assert_gate(1, "stale-baseline-entry: documented_op ")

    def test_a_fabricated_chunk_claim_reds_the_gate(self):
        self.stage({UNITS: registry("documented_op"), CHUNK: GHOST_CHUNK})
        self.assert_gate(1, "fabricated-name: ghost_op ")


@unittest.skipIf(BIN_MAY_BE_STALE,
                 "reify-audit may predate the path:name ledger grammar")
class LedgerGrammar(GateCase):

    def test_a_path_name_row_absorbs_its_fabrication(self):
        self.stage({
            UNITS: registry("documented_op"),
            CHUNK: GHOST_CHUNK,
            BASELINE: f"{GHOST_ROW}\n",
        })
        self.assert_gate(0)

    def test_a_path_name_row_whose_mention_is_gone_is_stale(self):
        self.stage({
            UNITS: registry("documented_op"),
            CHUNK: DOCUMENTED_CHUNK,
            BASELINE: f"{GHOST_ROW}\n",
        })
        self.assert_gate(1, f"stale-baseline-entry: {GHOST_ROW} ")

    def test_a_bare_row_for_an_undeclared_name_is_stale(self):
        self.stage({
            UNITS: registry("documented_op"),
            CHUNK: DOCUMENTED_CHUNK,
            BASELINE: "vanished_op\n",
        })
        self.assert_gate(1, "stale-baseline-entry: vanished_op ")


if __name__ == "__main__":
    unittest.main()
