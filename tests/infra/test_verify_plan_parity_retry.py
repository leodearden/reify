#!/usr/bin/env python3
"""
test_verify_plan_parity_retry.py — B1's failed_only retry member: bash
`scripts/verify.sh` and Python `scripts/verify.py` print the same test and all
plans under REIFY_VERIFY_RETRY_SCOPE=failed_only (S18 γ2, task 9099; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13).

A retry narrows each nextest profile pass to the exact test ids DF lists, but
only when the attempt-0 sidecar's tree_oid is the tree DF retries; whenever it
cannot narrow it says why on stderr and runs that pass in full. The
@@REIFY_RETRY_SCOPE@@ marker opens stdout iff some suite really narrowed.

Each case compares exit code, stdout and stderr byte for byte, after a
premise on bash's side (the marker line, the refusal lines in order, or a
fragment of the pass line), so a bash change that stops reaching the shape
reds here rather than passing vacuously.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_plan_parity_retry.sh.
"""

import sys
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from plan_parity_harness import (  # noqa: E402
    NEXTEST_ABSENT_PROBE,
    Case,
    PremisedParity,
    discard_clone,
    nextest_env,
    plan,
)

FAILED_ONLY = {"REIFY_VERIFY_RETRY_SCOPE": "failed_only"}
TREE = "b1-retry-tree"
WANTED = "b1-wanted-tree"
REFUSED = "verify.sh: retry refused: "
DRIFT = f"{REFUSED}tree drift — sidecar tree_oid "
MARKER = "@@REIFY_RETRY_SCOPE=failed_only@@ "
PLAN_HEADER = "# verify.sh plan — "
FILTER = "REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE"
ALPHA, BETA = "reify_doc::tests::alpha", "reify_doc::tests::beta"
SUBSET = f"test(={ALPHA}) | test(={BETA})"
ANY_SUBSET = "-E 'test(="
ATTEMPT_STAMP = 'printf \'{"tree_oid":"%s"'
NO_NEXTEST = f"{REFUSED}no nextest — cargo-test fallback has no -E filterset support (full verify)"
RELEASE_SKIP_MARKER = "echo 'RELEASE-PASS: skipped (delta-clean)'"
GUI_SPECS = "src/__tests__/a.test.ts src/__tests__/b.test.ts"
TEST_ALL = ("test", "--scope", "all")
BOTH = ("--profile", "both")


def tearDownModule():
    discard_clone()


def marker(debug=0, release=0, run_all=0, gui=0) -> str:
    return f"{MARKER}nextest_debug={debug} nextest_release={release} run_all={run_all} gui={gui}"


def no_subset(path: str, profile: str) -> str:
    return f"{REFUSED}no subset — filter file {path} absent/empty/unreadable (profile={profile}, full verify)"


class RetryParity(PremisedParity):
    def scratch_file(self, name: str, text: str) -> str:
        path = self.scratch / name
        path.write_bytes(text.encode("utf-8"))
        return str(path)

    def sidecar(
        self, text: str = f'{{"tree_oid":"{TREE}","profiles":"debug","timestamp":"t"}}\n', name="attempt.json"
    ) -> dict:
        """A failed_only retry whose attempt sidecar is the scratch file `name` holding `text`."""
        return {**FAILED_ONLY, "REIFY_VERIFY_ATTEMPT_SIDECAR": self.scratch_file(name, text)}

    def eligible(self, **env) -> dict:
        """A retry whose sidecar's tree_oid is the tree it retries, filtered by the base file of ALPHA and BETA."""
        base = self.scratch_file("filter.txt", f"{ALPHA}\n{BETA}\n")
        return {**self.sidecar(), "REIFY_VERIFY_RETRY_TREE_OID": TREE, FILTER: base, **env}


class TestRetryEligibility(RetryParity):
    """A retry narrows only when the attempt sidecar's tree_oid is REIFY_VERIFY_RETRY_TREE_OID."""

    def test_no_sidecar_and_no_tree_oid(self):
        self.check(Case(
            plan("no sidecar, tree oid unset", *TEST_ALL, env=FAILED_ONLY),
            first_line=PLAN_HEADER,
            stderr=(f"{DRIFT}<absent> != REIFY_VERIFY_RETRY_TREE_OID <unset> (full verify)",),
        ))

    def test_a_mismatched_tree_oid(self):
        self.check(Case(
            plan("sidecar tree_oid != the retried tree", *TEST_ALL,
                 env=self.eligible(REIFY_VERIFY_RETRY_TREE_OID=WANTED)),
            first_line=PLAN_HEADER, absent=(ANY_SUBSET,),
            stderr=(f"{DRIFT}{TREE} != REIFY_VERIFY_RETRY_TREE_OID {WANTED} (full verify)",),
        ))

    def test_a_sidecar_at_the_overridden_path(self):
        self.check(Case(
            plan("sidecar at REIFY_VERIFY_ATTEMPT_SIDECAR", *TEST_ALL, env=self.eligible()),
            first_line=marker(debug=2), stdout=(f"-E '{SUBSET}'",), stderr_absent=(REFUSED,),
        ))

    def test_the_tree_oid_extractor(self):
        shapes = (
            ("the key twice on one line", '{"tree_oid":"first","x":1,"tree_oid":"second"}\n', "second"),
            ("the key on a second line", '{"profiles":"debug",\n"tree_oid":"line-two"}\n', "line-two"),
            ("spaces around the colon", '{"tree_oid" : "spaced"}\n', "spaced"),
            ("an empty value", '{"tree_oid":""}\n', "<absent>"),
            ("not JSON", "not json at all\n", "<absent>"),
        )
        self.check(*(
            Case(
                plan(f"sidecar: {label}", *TEST_ALL,
                     env={**self.sidecar(text, f"attempt-{n}.json"), "REIFY_VERIFY_RETRY_TREE_OID": WANTED}),
                first_line=PLAN_HEADER,
                stderr=(f"{DRIFT}{extracted} != REIFY_VERIFY_RETRY_TREE_OID {WANTED} (full verify)",),
            )
            for n, (label, text, extracted) in enumerate(shapes)
        ))


class TestRetrySubsets(RetryParity):
    """Each profile pass narrows to its filter file's exact ids, or refuses and runs in full."""

    def test_the_base_file_under_both_profiles(self):
        self.check(
            *(
                Case(plan(f"{role} --profile both, base filter file", *TEST_ALL, *BOTH,
                          env=self.eligible(DF_VERIFY_ROLE=role)),
                     first_line=marker(debug=2, release=2), stdout=(f"-E '{SUBSET}'",), absent=(ATTEMPT_STAMP,))
                for role in ("task", "merge")
            ),
            Case(plan("merge, not a retry", *TEST_ALL, env={"DF_VERIFY_ROLE": "merge"}), stdout=(ATTEMPT_STAMP,)),
            Case(plan("all --scope all, base filter file", "all", "--scope", "all", env=self.eligible()),
                 first_line=marker(debug=2), stdout=(f"-E '{SUBSET}'",)),
        )

    def test_per_profile_files_override_the_base(self):
        debug = self.scratch_file("debug.txt", "reify_doc::tests::gamma\n")
        release = self.scratch_file("release.txt", "a::d\na::e\na::f\n")
        self.check(Case(
            plan("per-profile filter files", *TEST_ALL, *BOTH,
                 env=self.eligible(**{f"{FILTER}_DEBUG": debug, f"{FILTER}_RELEASE": release})),
            first_line=marker(debug=1, release=3), absent=(SUBSET,),
            stdout=("-E 'test(=reify_doc::tests::gamma)'", "-E 'test(=a::d) | test(=a::e) | test(=a::f)'"),
        ))

    def test_an_empty_profile_variable_falls_back_to_the_base(self):
        self.check(Case(
            plan("empty per-profile filter variables", *TEST_ALL, *BOTH,
                 env=self.eligible(**{f"{FILTER}_DEBUG": "", f"{FILTER}_RELEASE": ""})),
            first_line=marker(debug=2, release=2), stdout=(f"-E '{SUBSET}'",),
        ))

    def test_blank_padded_and_crlf_lines(self):
        ragged = self.scratch_file("ragged.txt", f"\n   \n  {ALPHA}\t\r\n\r\n{BETA}")
        self.check(Case(
            plan("blank, whitespace-only and CRLF lines, no final newline", *TEST_ALL,
                 env=self.eligible(**{FILTER: ragged})),
            first_line=marker(debug=2), stdout=(f"-E '{SUBSET}'",),
        ))

    def test_no_subset(self):
        empty = self.scratch_file("empty.txt", "")
        missing = str(self.scratch / "missing.txt")
        self.check(
            Case(plan("an empty filter file", *TEST_ALL, env=self.eligible(**{FILTER: empty})),
                 first_line=PLAN_HEADER, absent=(ANY_SUBSET,), stderr=(no_subset(empty, "debug"),)),
            Case(plan("a missing filter file", *TEST_ALL, env=self.eligible(**{FILTER: missing})),
                 first_line=PLAN_HEADER, stderr=(no_subset(missing, "debug"),)),
            Case(plan("every filter variable unset", *TEST_ALL, *BOTH, env=self.eligible(**{FILTER: None})),
                 first_line=PLAN_HEADER,
                 stderr=(no_subset("<unset>", "debug"), no_subset("<unset>", "release"))),
        )

    def test_a_subset_over_the_ceiling(self):
        self.check(Case(
            plan("REIFY_VERIFY_RETRY_MAX_SUBSET=1, two ids", *TEST_ALL,
                 env=self.eligible(REIFY_VERIFY_RETRY_MAX_SUBSET="1")),
            first_line=PLAN_HEADER, absent=(ANY_SUBSET,),
            stderr=(f"{REFUSED}subset too large — 2 > ceiling 1 (profile=debug, full verify)",),
        ))

    def test_the_subset_meets_the_heavy_filter(self):
        self.check(
            Case(plan("task, heavy excluded", *TEST_ALL, env=self.eligible(REIFY_GATE_EXCLUDE_HEAVY="1")),
                 first_line=marker(debug=2), stdout=("-E '(not (", f") & ({SUBSET})'"), absent=('-E "not (',)),
            Case(plan("offline --profile release", *TEST_ALL, "--profile", "release",
                      env=self.eligible(DF_VERIFY_ROLE="offline")),
                 first_line=marker(release=2),
                 stdout=(" --run-ignored all --no-fail-fast -E '((", f")) & ({SUBSET})'", "_confirm_rc"),
                 absent=('-E "(',)),
        )

    def test_the_subset_follows_test_threads(self):
        self.check(Case(
            plan("--test-threads=4", *TEST_ALL, "--test-threads=4", env=self.eligible()),
            first_line=marker(debug=2), stdout=(f" --test-threads=4 -E '{SUBSET}' --config-file ",),
        ))


class TestRetryAcrossTheRun(RetryParity):
    """What else a retry meets: no nextest, the merge role, other scopes, actions and suites."""

    def test_nextest_absent(self):
        env = {**nextest_env(self.scratch, "absent", NEXTEST_ABSENT_PROBE), **self.eligible()}
        self.check(Case(
            plan("nextest absent, --profile both", *TEST_ALL, *BOTH, env=env),
            first_line=PLAN_HEADER, stdout=("cargo test ",), stderr=(NO_NEXTEST, NO_NEXTEST),
        ))

    def test_a_skipped_release_pass_resolves_no_subset(self):
        env = self.eligible(**{
            "DF_VERIFY_ROLE": "merge", "REIFY_RELEASE_DELTA_SKIP": "1",
            "REIFY_AFFECTED_CRATES_OVERRIDE": "reify-doc", FILTER: None,
            f"{FILTER}_DEBUG": self.scratch_file("debug.txt", f"{ALPHA}\n{BETA}\n"),
        })
        self.check(Case(
            plan("merge, delta-clean release skip", *TEST_ALL, *BOTH, env=env),
            first_line=marker(debug=2), stdout=(RELEASE_SKIP_MARKER,), stderr_absent=("profile=release",),
        ))

    def test_a_full_retry_scope(self):
        self.check(Case(
            plan("REIFY_VERIFY_RETRY_SCOPE=full", *TEST_ALL, env=self.eligible(REIFY_VERIFY_RETRY_SCOPE="full")),
            first_line=PLAN_HEADER, absent=(ANY_SUBSET,), stderr_absent=(REFUSED,),
        ))

    def test_lint_has_no_test_region(self):
        self.check(Case(
            plan("lint under failed_only", "lint", "--scope", "all", env=FAILED_ONLY),
            first_line=PLAN_HEADER, stderr_absent=(REFUSED,),
        ))

    def test_the_marker_counts_every_suite(self):
        self.check(
            Case(plan("nextest and run_all subsets", *TEST_ALL,
                      env=self.eligible(REIFY_RUN_ALL_MEMBER_SUBSET="test_verify_policy.sh nope.sh")),
                 first_line=marker(debug=2, run_all=1)),
            Case(plan("nextest and gui subsets", *TEST_ALL, env=self.eligible(REIFY_GUI_RETRY_SPECS=GUI_SPECS)),
                 first_line=marker(debug=2, gui=2)),
            Case(plan("tree drift, gui subset only", *TEST_ALL,
                      env={**FAILED_ONLY, "REIFY_GUI_RETRY_SPECS": GUI_SPECS}),
                 first_line=marker(gui=2), stderr=(DRIFT,)),
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
