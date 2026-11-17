#!/usr/bin/env python3
"""
test_merge_verify_workspace_seam.py — stdlib unittest for the reify side of
dark-factory's merge-gate breadth seam (task 9871, esc-9761-1).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_merge_verify_workspace_seam.sh.

THE SEAM: top-level `merge_verify_workspace: true` in
dark-factory-orchestrator.yaml makes DF's merge role run the yaml's global
verify commands verbatim. The guard pins the knob, and checks the premise the
knob relies on: that global test command, under DF_VERIFY_ROLE=merge, plans the
whole-workspace nextest. That premise check plans the command in this process's
environment WITHOUT the yaml's `verify_env` overlay that DF adds, so it does not
see a verify_env entry that narrows the workspace pass.
Background: docs/notes/verify-pipeline-knobs.md.
"""

import json
import os
import re
import shlex
import subprocess
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ORCH_YAML = REPO_ROOT / "dark-factory-orchestrator.yaml"
KNOBS_DOC = "docs/notes/verify-pipeline-knobs.md"

WITHOUT_THE_KNOB = (
    "dark-factory-orchestrator.yaml must set top-level `merge_verify_workspace: true` "
    "(else the merge gate can run lint+type only); "
    f"see {KNOBS_DOC}."
)


def top_level_scalar(yaml_text, key):
    """The scalar value of the one column-0 `key:` line in yaml_text."""
    matches = re.findall(rf"^{re.escape(key)}:[ \t]*(.*)$", yaml_text, re.MULTILINE)
    if len(matches) != 1:
        raise AssertionError(
            f"expected exactly one top-level `{key}:` line, found {len(matches)}")
    raw = matches[0].strip()
    if not raw.startswith('"'):
        return re.sub(r"\s+#.*$", "", raw)
    value, end = json.JSONDecoder().raw_decode(raw)
    trailer = raw[end:].strip()
    if trailer and not trailer.startswith("#"):
        raise AssertionError(f"unexpected text after `{key}`'s quoted value: {trailer!r}")
    return value


class TestTopLevelScalar(unittest.TestCase):
    def test_quoted_value_is_decoded(self):
        text = 'a: 1\ntest_command: "./scripts/verify.sh test --scope branch"  # note\n'
        self.assertEqual(top_level_scalar(text, "test_command"),
                         "./scripts/verify.sh test --scope branch")

    def test_bare_value_drops_inline_comment(self):
        text = "merge_train_max_members: 3  # already the default\n"
        self.assertEqual(top_level_scalar(text, "merge_train_max_members"), "3")

    def test_indented_key_under_another_block_is_not_matched(self):
        text = "git:\n  merge_verify_workspace: true\n"
        with self.assertRaisesRegex(AssertionError, r"`merge_verify_workspace:`.*found 0"):
            top_level_scalar(text, "merge_verify_workspace")

    def test_duplicate_top_level_key_is_refused(self):
        text = "concurrent_verify: false\nconcurrent_verify: true\n"
        with self.assertRaisesRegex(AssertionError, r"`concurrent_verify:`.*found 2"):
            top_level_scalar(text, "concurrent_verify")


class TestMergeVerifyWorkspaceKnob(unittest.TestCase):
    def test_merge_verify_workspace_knob_is_set_true(self):
        try:
            value = top_level_scalar(ORCH_YAML.read_text(), "merge_verify_workspace")
        except AssertionError as unreadable:
            self.fail(f"{unreadable}. {WITHOUT_THE_KNOB}")
        self.assertEqual(value, "true", WITHOUT_THE_KNOB)


class TestGlobalTestCommandCoversWorkspace(unittest.TestCase):
    def test_merge_role_test_command_plans_whole_workspace_nextest(self):
        argv = shlex.split(top_level_scalar(ORCH_YAML.read_text(), "test_command"))
        result = subprocess.run(
            argv + ["--print-plan"],
            cwd=REPO_ROOT,
            env=dict(os.environ, DF_VERIFY_ROLE="merge"),
            capture_output=True,
            text=True,
            timeout=600,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

        lines = result.stdout.splitlines()
        headers = [line for line in lines if line.startswith("# verify.sh plan")]
        self.assertTrue(headers, f"no '# verify.sh plan' header in:\n{result.stdout}")
        header_fields = headers[0].split()
        self.assertIn("role=merge", header_fields, headers[0])
        self.assertIn("scope=all", header_fields, headers[0])

        commands = [line for line in lines if not line.startswith("#")]
        self.assertTrue(
            any(re.search(r"cargo nextest run --workspace\b", line) for line in commands),
            f"{shlex.join(argv)} under DF_VERIFY_ROLE=merge plans no whole-workspace "
            f"nextest, so running it verbatim at merge would not cover every crate:\n"
            f"{result.stdout}",
        )


if __name__ == "__main__":
    unittest.main()
