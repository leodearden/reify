#!/usr/bin/env python3
"""test_legibility_reify_config.py — TDD guard for reify's confusion-reduction
opt-in artifacts.

Task θ (§11) of the confusion-reduction PRD
(/home/leo/src/dark-factory/plans/confusion-reduction-prd.md): reify commits
two static artifacts (docs/legibility/legibility.yaml, docs/legibility/
confusion-codebook.yaml) that opt it into dark_factory's nightly trickle (ε)
and periodic census (η). This test exercises those committed artifacts
against the SHARED dark_factory `legibility` package
(scripts/legibility/{codebook,config,inventory}.py) rather than
reimplementing any of its schema/matching logic.

Co-located with reify's other scripts/test_*.py (stdlib unittest,
self-path-insert convention — see scripts/test_sn_gate.py). Mirrors
dark_factory's scripts/tests/test_codebook.py `_LIVE_CODEBOOK_PATH`
live-codebook validation pattern.

Cross-repo dependency: the dark-factory checkout is resolved via
DARK_FACTORY_ROOT (default /home/leo/src/dark-factory). If the `legibility`
package isn't importable there (checkout absent, or the confusion-reduction
PRD's scripts/legibility/ hasn't landed), every test in this module skips
rather than failing — see plan.json design_decisions for why this guard is
intentionally NOT registered in scripts/verify-pipeline-infra-tests.txt
(it would couple reify's hermetic cargo gate to an external checkout, and
the standing validation already runs continuously downstream: dark_factory's
merger validates on every nightly write, and θ′ validates at deploy time).

Run manually:
    python3 scripts/test_legibility_reify_config.py
"""
from __future__ import annotations

import os
import subprocess
import sys
import unittest
from pathlib import Path

import yaml

# reify repo root: scripts/test_legibility_reify_config.py -> scripts/ -> repo root
_REIFY_ROOT = Path(__file__).resolve().parent.parent
_LEGIBILITY_YAML = _REIFY_ROOT / "docs" / "legibility" / "legibility.yaml"
_CODEBOOK_YAML = _REIFY_ROOT / "docs" / "legibility" / "confusion-codebook.yaml"

_DARK_FACTORY_ROOT = Path(os.environ.get("DARK_FACTORY_ROOT", "/home/leo/src/dark-factory"))
_DARK_FACTORY_SCRIPTS = _DARK_FACTORY_ROOT / "scripts"
_DARK_FACTORY_CODEBOOK_PY = _DARK_FACTORY_SCRIPTS / "legibility" / "codebook.py"

if str(_DARK_FACTORY_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_DARK_FACTORY_SCRIPTS))

try:
    from legibility import codebook as _codebook_mod  # noqa: E402
    from legibility import inventory as _inventory_mod  # noqa: E402
    _LEGIBILITY_IMPORTABLE = True
except ImportError:
    _codebook_mod = None
    _inventory_mod = None
    _LEGIBILITY_IMPORTABLE = False


def _require_legibility_package() -> None:
    """Graceful cross-repo degrade: skip (never fail) when the external
    dark_factory `legibility` package isn't importable — this is a missing
    external dependency, not a defect in reify's own committed artifacts."""
    if not _LEGIBILITY_IMPORTABLE:
        raise unittest.SkipTest(
            "dark_factory 'legibility' package not importable from "
            f"{_DARK_FACTORY_SCRIPTS} (DARK_FACTORY_ROOT={_DARK_FACTORY_ROOT}); "
            "skipping cross-repo legibility validation."
        )


class TestLegibilityConfig(unittest.TestCase):
    """docs/legibility/legibility.yaml — per-project config (PRD §7.4).

    setUpClass only guards against the external package being absent
    (SkipTest). It deliberately does NOT guard against reify's own
    legibility.yaml being absent: a missing/malformed local artifact must
    surface as a real test FAILURE (open() raising), not a skip.
    """

    @classmethod
    def setUpClass(cls):
        _require_legibility_package()
        with open(_LEGIBILITY_YAML, encoding="utf-8") as f:
            cls.raw = yaml.safe_load(f)

    def test_required_fields(self):
        self.assertEqual(self.raw["project_id"], "reify")
        self.assertEqual(self.raw["project_root"], "/home/leo/src/reify")
        self.assertEqual(self.raw["escalation_port"], 8100)
        self.assertEqual(
            self.raw["cwd_prefixes"],
            [
                "/home/leo/src/reify",
                "/home/leo/src/warm-lanes",
                "/media/leo/data/lv-1/leo/reify",
            ],
        )

    def test_budgets_block_defaults(self):
        self.assertEqual(self.raw["budgets"]["max_daily_digest_bytes"], 300000)

    def test_sampling_block_defaults(self):
        self.assertEqual(self.raw["sampling"]["top_fraction"], 0.12)
        self.assertEqual(self.raw["sampling"]["per_stratum_min"], 2)

    def test_census_block_defaults(self):
        census = self.raw["census"]
        self.assertEqual(census["max_interval_days"], 10)
        self.assertEqual(census["tasks_landed_threshold"], 120)
        self.assertEqual(census["tasks_landed_min_days"], 7)
        self.assertEqual(census["novelty_spike"]["count"], 4)
        self.assertEqual(census["novelty_spike"]["window_hours"], 72)
        self.assertEqual(census["floor_days"], 5)
        self.assertEqual(census["saturation"]["dup_rate"], 0.9)
        self.assertEqual(census["saturation"]["consecutive_batches"], 2)

    def test_models_block_defaults(self):
        models = self.raw["models"]
        self.assertEqual(models["trickle"], "haiku")
        self.assertEqual(models["census_miner"], "sonnet")
        self.assertEqual(models["census_verify"], "sonnet")
        self.assertEqual(models["census_synthesis"], "fable")

    def test_loads_via_shared_pydantic_config(self):
        """legibility.config.load_config (the shared §7.4 pydantic schema)
        must accept reify's file without raising ValidationError."""
        from legibility.config import load_config

        cfg = load_config(_LEGIBILITY_YAML)
        self.assertEqual(cfg.project_id, "reify")
        self.assertEqual(cfg.project_root, "/home/leo/src/reify")
        self.assertEqual(cfg.escalation_port, 8100)
        self.assertEqual(
            cfg.cwd_prefixes,
            [
                "/home/leo/src/reify",
                "/home/leo/src/warm-lanes",
                "/media/leo/data/lv-1/leo/reify",
            ],
        )


class TestCwdPrefixCoverage(unittest.TestCase):
    """cwd_prefix coverage, proven via the SHARED is_member (real
    Path.is_relative_to path-component semantics — not a reimplementation).
    """

    # Six representative encoded-dir shapes actually observed under
    # ~/.claude/projects (see plan.json analysis: 36 + 48 + 191 = 275).
    REIFY_CWDS = [
        "/home/leo/src/reify",
        "/home/leo/src/reify/.worktrees/1447",
        "/home/leo/src/reify/.claude-worktrees/foo",
        "/home/leo/src/warm-lanes/worktrees/_lane-9",
        "/home/leo/src/warm-lanes/worktrees/1450",
        "/media/leo/data/lv-1/leo/reify/build-worktrees/12",
    ]

    # Sibling cwds that must NOT match — proves path-component precision
    # (a raw string-prefix match on the lossy encoded name would wrongly
    # include these).
    NON_REIFY_CWDS = [
        "/home/leo/src/dark-factory",
        "/home/leo/src/reify-cockpit",
    ]

    @classmethod
    def setUpClass(cls):
        _require_legibility_package()
        with open(_LEGIBILITY_YAML, encoding="utf-8") as f:
            cls.cwd_prefixes = yaml.safe_load(f)["cwd_prefixes"]

    def test_reify_cwds_are_members(self):
        for cwd in self.REIFY_CWDS:
            with self.subTest(cwd=cwd):
                self.assertTrue(
                    _inventory_mod.is_member(cwd, self.cwd_prefixes),
                    f"{cwd} should be a member of {self.cwd_prefixes}",
                )

    def test_non_reify_cwds_are_excluded(self):
        for cwd in self.NON_REIFY_CWDS:
            with self.subTest(cwd=cwd):
                self.assertFalse(
                    _inventory_mod.is_member(cwd, self.cwd_prefixes),
                    f"{cwd} should NOT be a member of {self.cwd_prefixes}",
                )


class TestConfusionCodebook(unittest.TestCase):
    """docs/legibility/confusion-codebook.yaml — seeded, empty v2 codebook.

    Exercises the SHARED dark_factory validator (both as a library call and
    as the literal `codebook.py validate` CLI the task's acceptance signal
    names) against reify's committed codebook, rather than reimplementing
    the v2 schema check.
    """

    @classmethod
    def setUpClass(cls):
        _require_legibility_package()
        # Deliberately NOT guarded against _CODEBOOK_YAML being absent —
        # see TestLegibilityConfig's docstring: a missing local artifact
        # must be a real failure, not a skip.
        cls.codebook = _codebook_mod.load(_CODEBOOK_YAML)

    def test_validate_returns_no_errors(self):
        self.assertEqual(_codebook_mod.validate(self.codebook), [])

    def test_seeded_v2_shape(self):
        self.assertEqual(self.codebook["version"], 2)
        self.assertEqual(self.codebook["entries"], [])
        self.assertEqual(self.codebook["candidates"], [])

    def test_validator_cli_exits_zero(self):
        """The task's literal user-observable acceptance signal: dark_factory's
        shared validator CLI exits green on reify's committed codebook."""
        result = subprocess.run(
            [sys.executable, str(_DARK_FACTORY_CODEBOOK_PY), "validate", str(_CODEBOOK_YAML)],
            capture_output=True,
            text=True,
        )
        self.assertEqual(
            result.returncode, 0,
            f"codebook.py validate exited {result.returncode}; stderr:\n{result.stderr}",
        )


if __name__ == "__main__":
    unittest.main()
