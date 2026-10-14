#!/usr/bin/env python3
"""
Regression guard for task 4966: knob-sensitive tests/infra self-suites must
behave identically under the production dark-factory-orchestrator.yaml
verify_env ambient, not just in a clean default shell.

Root cause this guards against: the verify_env block exports
REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 and REIFY_GATE_EXCLUDE_HEAVY=1 (plus
sccache/incremental/semaphore/PSI knobs) into the whole verify.sh process tree
by design. Deploy 65b8412206 flipped both knobs to "1" and broke two infra
self-suites in one night — each caught only at L2 after a multi-hour debugger
loop, because both suites were GREEN standalone (a bare invocation never sets
the ambient) and only RED in-pipeline. Task 4961 guarded test_run_all.sh; task
4965 fixed the resulting shell-quoting bug in test_occt_flock_gate.sh. This
file is the general drift-guard: it extracts the FULL verify_env export set
directly from dark-factory-orchestrator.yaml — the single source the
orchestrator itself injects from, so there is no second manifest to drift out
of sync — and re-runs a knob-sensitive suite once under that exact ambient.

PORTED FROM BASH (task 7430). This was a 436-line test_*.sh; it is now the
first port under docs/notes/infra-test-bash-to-python-migration-policy.md,
triggered by the policy's own cadence — a bash member is ported the next time
it is touched to fix a flake, and this file is flaky-ledger rank #3. Its
`.sh` sibling of the same basename is now the thin wrapper that run_all.sh
discovers. Two deliberate departures from a transliteration:

  * The verify_env extractor was an embedded awk program emitting `KEY=VALUE`
    text that the caller re-split. It is now a parser returning a dict — the
    ad-hoc string interchange is gone, which is the point of the port.
  * The bash original carried a self-guard asserting the file installs exactly
    one `trap ... EXIT`, because a second `trap ... EXIT` SILENTLY replaces the
    first and leaks whatever the replaced handler owned (which had really
    happened here). That assertion is deliberately DROPPED rather than
    transliterated: unittest's addCleanup registers cleanups on a stack and
    runs all of them, so the failure mode the self-guard existed to detect
    cannot occur. Porting the assertion would pin a property of bash, not of
    this test.
  * Two STRENGTHENINGS, declared here rather than smuggled in, because the
    policy asks a port to be behaviour-preserving and these are the places it
    deliberately is not. The bash original asserted only that its SIGTERM-deaf
    fixture surfaced as 137; this file also asserts that the fixture's recorded
    pid is GONE, since 137 alone would be reported just the same by a backstop
    that CLAIMED a kill it never performed. And the post-SIGKILL drain is
    bounded, with a test for the abandonment branch — a Python-only hazard the
    bash original could not have had, because `timeout` never owned a pipe to
    block on. Both are argued at their sites.
"""

import os
import re
import signal
import subprocess
import sys
import tempfile
import unittest
from dataclasses import dataclass
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
ORCHESTRATOR_YAML = REPO_ROOT / "dark-factory-orchestrator.yaml"
NESTED_SUITE = SCRIPT_DIR / "test_occt_flock_gate.sh"
NESTED_SUITE_NAME = NESTED_SUITE.name

# Generous by design: the nested suite has measured 16-246s on this host, and
# the whole of run_all.sh runs under a 30m outer envelope. 900s never
# discriminates against a slow-but-alive run, yet still fires far enough inside
# that envelope for the outcome to be attributed to this suite rather than
# surfacing as "run_all was interrupted". A BROKEN-INFRA BACKSTOP, not a timing
# assertion: every assertion below is on an exit CODE, never on a magnitude.
NESTED_BACKSTOP_SECS = 900
# SIGTERM-to-SIGKILL grace, matching the house convention (`timeout
# --kill-after=60` in verify.sh and test_occt_flock_gate.sh). Only ever reached
# by a child that already failed to answer SIGTERM.
NESTED_KILL_GRACE_SECS = 60

# Appended to a wedged run's output when even the post-SIGKILL drain had to be
# abandoned. A SENTINEL LINE rather than a fourth invented exit code: the group
# really was SIGKILLed, so RC_WEDGED_SIGKILL stays the honest diagnosis, and
# what a reader actually needs is to know that output stops here.
ABANDONED_DRAIN_NOTE = (
    "\n[backstop] drain abandoned: a descendant outlived the process group and "
    "still holds the output pipe. Anything written after this point is unread."
    "\n")

# Exit codes run_under_ambient invents when the child itself did not exit.
RC_WEDGED_SIGTERM = 124
RC_WEDGED_SIGKILL = 137
RC_AMBIENT_NOT_APPLIED = 99

# The knob whose presence proves the hostile ambient actually reached the child.
AMBIENT_PROOF_KEY = "REIFY_GATE_EXCLUDE_HEAVY"
AMBIENT_PROOF_VALUE = "1"

# Only the nested suite's OWN test_summary line qualifies; anchored so an inner
# mock's "0 failed"-shaped output can never false-pass.
CLEAN_SUMMARY_RE = re.compile(r"^Results: \d+ passed, 0 failed$", re.MULTILINE)

_VERIFY_ENV_HEADER_RE = re.compile(r"^verify_env:[ \t]*$")
_BLOCK_END_RE = re.compile(r"^[^\s#]")
_ENTRY_RE = re.compile(r"^[ \t]+([A-Za-z_][A-Za-z0-9_]*):(.*)$")


def verify_env_exports(yaml_path):
    """The `verify_env:` block of an orchestrator YAML, as {KEY: VALUE}.

    Deliberately not a YAML library call: this must agree with what the
    orchestrator injects, and the block is a flat scalar map, so a targeted
    reader keeps the dependency set empty (stdlib only, like every other
    Python member here).

    Block entry is a top-level `verify_env:` line. A later top-level
    non-comment line ends the block, so trailing top-level `#` comments (the
    jobserver-wiring note preceding the real file's `jobserver:` block) do not
    end it early. In-block blank and comment lines are skipped. A double-quoted
    value is taken verbatim between the first pair of quotes; a bare value is
    reduced to its first whitespace-delimited token, dropping any trailing
    inline comment.
    """
    exports = {}
    in_block = False
    for line in Path(yaml_path).read_text().splitlines():
        if not in_block:
            if _VERIFY_ENV_HEADER_RE.match(line):
                in_block = True
            continue
        if _BLOCK_END_RE.match(line):
            break
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        entry = _ENTRY_RE.match(line)
        if entry:
            exports[entry.group(1)] = _parse_value(entry.group(2))
    return exports


def _parse_value(rest):
    rest = rest.lstrip()
    if rest.startswith('"'):
        tail = rest[1:]
        end = tail.find('"')
        return tail if end < 0 else tail[:end]
    tokens = rest.split()
    return tokens[0] if tokens else ""


@dataclass(frozen=True)
class AmbientRun:
    """What the bounded nested run produced.

    `rc` is the child's own exit code, or one of RC_WEDGED_SIGTERM /
    RC_WEDGED_SIGKILL / RC_AMBIENT_NOT_APPLIED — codes this module invents for
    outcomes where the child did not exit on its own terms.
    """
    rc: int
    output: str


def run_under_ambient(yaml_path, budget_secs, grace_secs, cmd, base_env=None):
    """Run `cmd` under the production verify_env ambient, bounded.

    WHY THE SIGKILL ESCALATION. A lone SIGTERM leaves a child that blocks or
    ignores it running, which defeats the one job a backstop has. The
    escalation is the house convention (`timeout --kill-after`) for exactly
    that reason.

    WHY EVERY RUNG WAITS, INCLUDING THE LAST ONE. `grace_secs` bounds the drain
    after SIGKILL exactly as it bounds the drain after SIGTERM, and that
    symmetry is the fix for a hazard this backstop would otherwise have had for
    itself: os.killpg reaches the process GROUP, so a descendant that called
    setsid/setpgid survives it and keeps the inherited write end of the stdout
    pipe open. An unbounded `communicate()` on the last rung therefore blocks
    on a process the kill never touched -- the backstop becoming the wedge it
    exists to prevent, with the outer run_all envelope once again the only
    thing that ends this file. MEASURED, not reasoned: a fixture that
    backgrounds `setsid sleep ...` really does hold the pipe past a group
    SIGKILL, and
    test_a_descendant_that_outlives_the_group_does_not_wedge_the_backstop
    exercises that branch against this real code path.

    WHY A NEW SESSION AND killpg, NOT proc.kill(). start_new_session puts the
    child in its own process group so the signal reaches the GROUP: the nested
    suite's backgrounded descendants — gated flock holders, wrapper
    invocations, each with its own generous budget — are reaped WITH it instead
    of being orphaned into the reaper's lap. This is what `timeout`'s default
    (non---foreground) mode did for the bash original.

    NON-VACUITY PREFLIGHT. If the assembled ambient does not carry the hostile
    knob, the run would prove nothing either way, so it is not attempted at all
    and RC_AMBIENT_NOT_APPLIED is returned. The check is on the ASSEMBLED env
    (the one the child would actually see), not on the extracted exports alone,
    which is why `base_env` is injectable: a test that needs the negative case
    must be able to withhold an inherited knob deterministically.

    stdout and stderr are MERGED here, unconditionally. The bash original left
    the merge to each call site so that test_slot_timeout_marker.sh's Section G
    could see the redirect; here the choice is made once, in the single spawn
    funnel, where it is visible to a reader. That reader is not hypothetical:
    since task #7626 Section G reads THIS call's kwargs directly, and
    `stderr=subprocess.STDOUT` counts as a diversion only because
    `stdout=subprocess.PIPE` is on the same call. Deleting either one turns
    G1 RED for this member.
    """
    env = dict(os.environ if base_env is None else base_env)
    env.update(verify_env_exports(yaml_path))
    if env.get(AMBIENT_PROOF_KEY) != AMBIENT_PROOF_VALUE:
        return AmbientRun(rc=RC_AMBIENT_NOT_APPLIED, output="AMBIENT-NOT-APPLIED")

    proc = subprocess.Popen(
        cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, errors="replace", start_new_session=True,
    )
    try:
        output, _ = proc.communicate(timeout=budget_secs)
        return AmbientRun(rc=proc.returncode, output=output)
    except subprocess.TimeoutExpired:
        pass

    drained, output = _signal_group_and_drain(proc, signal.SIGTERM, grace_secs)
    if drained:
        return AmbientRun(rc=RC_WEDGED_SIGTERM, output=output)
    drained, output = _signal_group_and_drain(proc, signal.SIGKILL, grace_secs)
    if not drained:
        _abandon_pipe(proc)
        output += ABANDONED_DRAIN_NOTE
    return AmbientRun(rc=RC_WEDGED_SIGKILL, output=output)


def _signal_group_and_drain(proc, sig, grace_secs):
    """Signal the child's whole group, then drain its pipe under `grace_secs`.

    Returns `(drained, output)`. `drained` is False when the pipe outlived the
    grace -- the SIGTERM caller's cue to escalate, and the SIGKILL caller's cue
    that there is nothing left to escalate TO. The output comes back either
    way: TimeoutExpired carries everything read so far, and a retried
    `communicate()` keeps accumulating into the same buffer (MEASURED on this
    host's CPython), so an abandoned run still reports what the child managed
    to say rather than throwing that attribution away.
    """
    try:
        os.killpg(os.getpgid(proc.pid), sig)
    except ProcessLookupError:
        pass
    try:
        output, _ = proc.communicate(timeout=grace_secs)
        return True, output
    except subprocess.TimeoutExpired as expired:
        return False, _partial_output(expired)


def _partial_output(expired):
    """What the child managed to say before a drain gave up.

    TimeoutExpired carries those bytes RAW even when the Popen was opened in
    text mode (MEASURED, not assumed), so the decode happens here rather than
    leaking bytes into an AmbientRun whose `output` is documented as str.
    """
    if expired.output is None:
        return ""
    if isinstance(expired.output, bytes):
        return expired.output.decode(errors="replace")
    return expired.output


def _abandon_pipe(proc):
    """Release this process's end of a pipe a descendant is still holding.

    The read end is the only half this process owns; the escapee owns the write
    end and is the orphan reaper's problem
    (docs/notes/orphaned-test-binary-reaper.md), not this backstop's. poll()
    then reaps the direct child -- already ended by the group SIGKILL -- so
    neither a zombie nor a "subprocess still running" warning outlives the run.
    """
    if proc.stdout is not None:
        proc.stdout.close()
    proc.poll()


@dataclass(frozen=True)
class Verdict:
    """What an rc MEANS for the nested suite.

    The three non-zero outcomes are three DIFFERENT diagnoses and must never be
    read as one another: a wedge is an infrastructure hang, a completed failure
    is a regression, and a missing ambient means the run settled nothing.
    `outcome` is a token rather than prose so a reader — or a later classifier —
    reaches the diagnosis without parsing a sentence.
    """
    outcome: str
    rc: int
    ok: bool
    message: str


def nested_verdict(rc, suite):
    detail = {
        0: ("passed", ""),
        RC_WEDGED_SIGTERM: (
            "wedged",
            f" -- it never finished and the anti-hang backstop ended it with "
            f"SIGTERM. That is a HANG in {suite}, not a failed assertion inside "
            f"it: look for a barrier that never released, not for a regression."),
        RC_WEDGED_SIGKILL: (
            "wedged",
            f" -- it never finished AND did not answer SIGTERM, so the backstop "
            f"escalated to SIGKILL. Still a HANG in {suite}, not a failed "
            f"assertion: look for a child that blocks or ignores SIGTERM. (An "
            f"external SIGKILL, e.g. the OOM killer, lands here too -- also "
            f"infrastructure, never a regression.)"),
        RC_AMBIENT_NOT_APPLIED: (
            "ambient-not-applied",
            " -- the hostile verify_env ambient was not in effect, so this run "
            "proves nothing either way."),
    }
    outcome, tail = detail.get(
        rc,
        ("failed",
         f" -- it ran to completion and reported failures. That is a REGRESSION "
         f"in {suite}, not a hang."),
    )
    return Verdict(
        outcome=outcome, rc=rc, ok=(rc == 0),
        message=f"nested={suite} outcome={outcome} rc={rc}{tail}",
    )


def process_alive(pid):
    """Signal 0 probes for existence without delivering anything."""
    try:
        os.kill(pid, 0)
    except (ProcessLookupError, PermissionError):
        return False
    return True


def quote_child_output(text):
    """Prefix every line with a NON-whitespace marker before echoing a nested
    child's output.

    dark-factory's wedge classifier is anchored `^[ \\t]*@@REIFY_SLOT_TIMEOUT@@`,
    so indentation alone does NOT defeat it — only a non-whitespace prefix does.
    test_occt_flock_gate.sh reaches a real REIFY_OCCT_LOCK_WAIT deadline and
    emits a column-0 sentinel of exactly that shape; echoed raw, a wedge
    sentinel from the CHILD would be attributed to THIS run. Same prefix, and
    the same reason, as test_helpers.sh's _assert_emit_desc (task 6353);
    behavioural pin: test_slot_timeout_marker.sh E4.
    """
    return "\n".join(f"  | {line}" for line in text.splitlines())


# ---------------------------------------------------------------------------
# Extractor correctness (a): synthetic fixture.
# ---------------------------------------------------------------------------

SYNTHETIC_YAML = """\
pre_block_key: pre_block_value
verify_env:
  ALPHA: "1"
  BETA: bare_value
  # in-block comment line, must be skipped
  GAMMA: "unlimited"
next_block:
  child_key: child_value
"""


class FixtureBase(unittest.TestCase):

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write_yaml(self, text, name="fixture.yaml"):
        path = self.tmpdir / name
        path.write_text(text)
        return path

    def fixture_suite(self, *lines, name="suite.sh"):
        """A throwaway stand-in for the nested suite."""
        path = self.tmpdir / name
        path.write_text("#!/usr/bin/env bash\n" + "".join(f"{l}\n" for l in lines))
        path.chmod(0o755)
        return path


class TestExtractorSynthetic(FixtureBase):
    """Block entry, top-level block exit, quoted and bare values, an in-block
    comment and a blank line — independent of what the real YAML holds today."""

    def setUp(self):
        super().setUp()
        self.exports = verify_env_exports(self.write_yaml(SYNTHETIC_YAML))

    def test_emits_exactly_the_in_block_keys(self):
        self.assertEqual(
            self.exports,
            {"ALPHA": "1", "BETA": "bare_value", "GAMMA": "unlimited"},
        )

    def test_does_not_leak_the_pre_block_top_level_key(self):
        self.assertNotIn("pre_block_key", self.exports)

    def test_does_not_leak_the_next_block_child_key(self):
        self.assertNotIn("child_key", self.exports)

    def test_does_not_leak_the_in_block_comment_line(self):
        self.assertFalse(
            [k for k in self.exports if "comment" in k.lower()],
            f"a comment line became a key: {self.exports}",
        )

    def test_bare_value_drops_a_trailing_inline_comment(self):
        exports = verify_env_exports(self.write_yaml(
            "verify_env:\n  DELTA: bare_tok   # trailing note\n", name="inline.yaml"))
        self.assertEqual(exports, {"DELTA": "bare_tok"})

    def test_blank_line_inside_the_block_does_not_end_it(self):
        exports = verify_env_exports(self.write_yaml(
            'verify_env:\n  ONE: "1"\n\n  TWO: "2"\n', name="blank.yaml"))
        self.assertEqual(exports, {"ONE": "1", "TWO": "2"})

    def test_top_level_comment_after_the_block_does_not_end_it_early(self):
        exports = verify_env_exports(self.write_yaml(
            'verify_env:\n  ONE: "1"\n# top-level note\n  TWO: "2"\n'
            'jobserver:\n  enabled: true\n', name="tlcomment.yaml"))
        self.assertEqual(exports, {"ONE": "1", "TWO": "2"})

    def test_a_quoted_value_keeps_its_inner_whitespace(self):
        exports = verify_env_exports(self.write_yaml(
            'verify_env:\n  PATHY: "/a b/c.jsonl"\n', name="quoted.yaml"))
        self.assertEqual(exports, {"PATHY": "/a b/c.jsonl"})

    def test_a_yaml_with_no_verify_env_block_yields_nothing(self):
        exports = verify_env_exports(self.write_yaml(
            "other:\n  key: value\n", name="noblock.yaml"))
        self.assertEqual(exports, {})


class TestExtractorRealYaml(unittest.TestCase):
    """Non-vacuity: the emitted set really carries the two knobs that caused
    the escapes, and block exit really stops before the following `jobserver:`
    block's `enabled: true` — a real adjacent top-level key today."""

    @classmethod
    def setUpClass(cls):
        if not ORCHESTRATOR_YAML.is_file():
            raise unittest.SkipTest(f"not found: {ORCHESTRATOR_YAML}")
        cls.exports = verify_env_exports(ORCHESTRATOR_YAML)

    def test_carries_the_gate_exclude_heavy_knob(self):
        self.assertEqual(self.exports.get("REIFY_GATE_EXCLUDE_HEAVY"), "1")

    def test_carries_the_exclude_host_infra_knob(self):
        self.assertEqual(self.exports.get("REIFY_RUN_ALL_EXCLUDE_HOST_INFRA"), "1")

    def test_does_not_leak_the_jobserver_block(self):
        self.assertNotIn("enabled", self.exports)

    def test_is_non_empty_and_every_key_is_a_well_formed_env_name(self):
        self.assertTrue(self.exports)
        bad = [k for k in self.exports if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", k)]
        self.assertEqual(bad, [], f"malformed export keys: {bad}")


# ---------------------------------------------------------------------------
# The nested run is BOUNDED and its outcomes are DISTINGUISHABLE (task 6247).
#
# Unbounded, a wedge inside the nested suite wedges this file too, until the
# outer `timeout --kill-after=60 30m` envelope kills the whole of run_all.sh --
# at which point the attribution is gone and the failure reads as "run_all was
# interrupted", not "the nested suite hung". And a wedge would arrive through
# the same non-zero channel as a suite that ran to completion and failed an
# assertion, which are opposite diagnoses.
#
# run_under_ambient is the whole nested run behind one interface, so the wedge
# path is exercised here against the REAL code path with tiny fixture suites
# rather than against a stub, or left untested until it happens for real.
# ---------------------------------------------------------------------------

class TestNestedBackstop(FixtureBase):

    def test_a_wedged_suite_surfaces_as_124(self):
        """Instead of hanging this file until the outer envelope kills run_all."""
        suite = self.fixture_suite("sleep 600")
        run = run_under_ambient(ORCHESTRATOR_YAML, 2, 2, ["bash", str(suite)])
        self.assertEqual(run.rc, RC_WEDGED_SIGTERM)

    def test_a_sigterm_deaf_suite_is_still_ended_and_surfaces_as_137(self):
        """The half a lone SIGTERM cannot do: a child that IGNORES it would
        leave the backstop itself hanging forever.

        The exit code alone is too weak a pin -- it would still be 137 if the
        backstop gave up and merely REPORTED a kill it never performed, leaking
        the process. Mutation-checked: replacing the SIGKILL escalation with a
        second SIGTERM passes the code assertion and fails this one.
        """
        pidfile = self.tmpdir / "deaf.pid"
        suite = self.fixture_suite(
            "trap '' TERM", f'echo $$ > "{pidfile}"', "sleep 600")
        run = run_under_ambient(ORCHESTRATOR_YAML, 2, 2, ["bash", str(suite)])
        recorded = pidfile.read_text().strip() if pidfile.is_file() else ""
        # Checked BEFORE the rc assertion, and on purpose. A bash that has not
        # reached `echo $$` under process-spawn and scheduling latency leaves
        # this file unwritten, which an unguarded int() turns into an opaque
        # FileNotFoundError/ValueError in the member that is flaky-ledger
        # rank #3. It also makes the rc assertion below MISLEADING: a bash that
        # had not yet installed `trap '' TERM` would have died to the SIGTERM
        # and returned 124, so "expected 137, got 124" would name the wrong
        # defect. Attribution first, then the assertion it guards.
        self.assertRegex(
            recorded, r"^\d+$",
            f"the deaf fixture recorded no usable pid ({recorded!r}) inside its "
            "budget -- that is a spawn-latency artefact of this fixture, not a "
            "backstop bug, and it invalidates the exit-code assertion that "
            "follows")
        self.assertEqual(run.rc, RC_WEDGED_SIGKILL)
        self.assertFalse(process_alive(int(recorded)),
                         f"pid {recorded} outlived the backstop's escalation")

    def test_a_descendant_that_outlives_the_group_does_not_wedge_the_backstop(self):
        """The backstop's own last wedge, closed.

        os.killpg reaches the process GROUP. A descendant that called setsid is
        in another group, survives the SIGKILL, and keeps the inherited write
        end of the stdout pipe open -- so an UNBOUNDED post-SIGKILL drain
        blocks on a process the kill never touched. Mutation-checked: restore
        `grace_secs=None` on that rung and this test never returns.

        THE FIXTURE'S ESCAPEE IS TIED TO THE TMPDIR, not to a sleep long enough
        to win a race. It holds the pipe for exactly as long as this test's own
        fixture directory exists, so it outlives the drain deterministically in
        one direction and is gone within a second of setUp's cleanup in the
        other -- no timing assumption, and no orphan left behind for the
        reaper. (A host without `setsid` reds here loudly rather than passing
        vacuously: the descendant would stay in the group, the drain would
        succeed, and the note assertion would fail.)

        Asserted on an exit CODE and on output content, never on a magnitude,
        like every other assertion in this file.
        """
        held_open = f'while [ -d "{self.tmpdir}" ]; do sleep 1; done'
        suite = self.fixture_suite(
            f"setsid bash -c '{held_open}' &",
            'echo "ESCAPEE-SPAWNED"',
            "sleep 600")
        run = run_under_ambient(ORCHESTRATOR_YAML, 2, 2, ["bash", str(suite)])
        self.assertEqual(run.rc, RC_WEDGED_SIGKILL)
        self.assertIn("drain abandoned", run.output,
                      "the drain returned without abandoning, so this fixture "
                      "did not reproduce the escaped-descendant wedge and the "
                      "branch under test was never reached")
        self.assertIn(
            "ESCAPEE-SPAWNED", run.output,
            "what the child DID say must survive the abandonment -- "
            "attribution is the whole job of a backstop")

    def test_a_suite_that_ran_and_failed_passes_its_own_code_through(self):
        """The discriminator. Without it the backstop could "pass" by mapping
        every non-zero outcome onto 124, erasing the distinction it exists to
        make."""
        suite = self.fixture_suite("exit 3")
        run = run_under_ambient(ORCHESTRATOR_YAML, 60, 60, ["bash", str(suite)])
        self.assertEqual(run.rc, 3)

    def test_a_suite_that_passed_reports_zero_through_the_backstop(self):
        suite = self.fixture_suite('echo "Results: 1 passed, 0 failed"')
        run = run_under_ambient(ORCHESTRATOR_YAML, 60, 60, ["bash", str(suite)])
        self.assertEqual(run.rc, 0)

    def test_the_production_ambient_genuinely_reaches_the_nested_child(self):
        """Without this the verdicts above are all about a run that proves
        nothing."""
        suite = self.fixture_suite(
            f'echo "HEAVY=${{{AMBIENT_PROOF_KEY}:-unset}}"')
        run = run_under_ambient(ORCHESTRATOR_YAML, 60, 60, ["bash", str(suite)])
        self.assertEqual(run.rc, 0)
        self.assertIn(f"HEAVY={AMBIENT_PROOF_VALUE}", run.output.splitlines())

    def test_a_missing_ambient_is_refused_with_99_and_the_child_never_runs(self):
        """The base env is withheld explicitly: under the real gate this
        process ALREADY inherits the knob, so a fixture YAML that merely omits
        it would still assemble a hostile ambient and pass for the wrong
        reason."""
        marker = self.tmpdir / "child-ran"
        suite = self.fixture_suite(f'touch "{marker}"')
        bare_yaml = self.write_yaml("verify_env:\n  UNRELATED: \"1\"\n",
                                    name="no-knob.yaml")
        run = run_under_ambient(bare_yaml, 60, 60, ["bash", str(suite)],
                                base_env={})
        self.assertEqual(run.rc, RC_AMBIENT_NOT_APPLIED)
        self.assertFalse(marker.exists(), "the child ran despite no ambient")


class TestVerdictMapping(unittest.TestCase):
    """Each rc must carry the right success/failure sense and name its own
    diagnosis."""

    def _check(self, rc, expect_ok, *needles):
        verdict = nested_verdict(rc, NESTED_SUITE_NAME)
        self.assertEqual(verdict.ok, expect_ok, verdict.message)
        for needle in (NESTED_SUITE_NAME,) + needles:
            self.assertIn(needle, verdict.message)

    def test_124_names_the_suite_and_calls_it_a_wedge(self):
        self._check(RC_WEDGED_SIGTERM, False, "wedged")

    def test_137_is_a_wedge_too_so_a_sigterm_deaf_hang_is_never_a_regression(self):
        self._check(RC_WEDGED_SIGKILL, False, "wedged")

    def test_3_says_the_suite_ran_and_failed_not_that_it_wedged(self):
        self._check(3, False, "failed")
        self.assertNotIn("wedged", nested_verdict(3, NESTED_SUITE_NAME).message)

    def test_99_names_the_ambient_not_applied_preflight(self):
        self._check(RC_AMBIENT_NOT_APPLIED, False, "ambient-not-applied")

    def test_0_is_a_pass(self):
        self._check(0, True, "passed")


# ---------------------------------------------------------------------------
# End-to-end: the REAL test_occt_flock_gate.sh under the REAL production
# ambient, exactly once.
#
# Mirrors test_run_all_ambient_isolation.sh (task 4961)'s run-the-real-suite-
# once idiom, generalized from one hardcoded knob to the FULL verify_env set.
#
# Non-vacuity: post-4965 the nested suite exits 0 under BOTH the default env
# and this ambient, so its exit code alone cannot prove the ambient applied.
# run_under_ambient's preflight refuses to start the child at all unless the
# hostile knob is present, so "occt exits 0 under a PROVABLY hostile ambient"
# is the real, non-vacuous claim.
# ---------------------------------------------------------------------------

class TestNestedSuiteUnderRealAmbient(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        if not NESTED_SUITE.is_file():
            raise unittest.SkipTest(f"not found: {NESTED_SUITE}")
        cls.ambient = run_under_ambient(
            ORCHESTRATOR_YAML, NESTED_BACKSTOP_SECS, NESTED_KILL_GRACE_SECS,
            ["bash", str(NESTED_SUITE)],
        )
        cls.verdict = nested_verdict(cls.ambient.rc, NESTED_SUITE_NAME)
        # Emitted BEFORE any assertion, so a wedge is attributed even to a
        # reader who sees nothing but this file's own output.
        print(cls.verdict.message, file=sys.stderr)

    def test_exits_zero_under_the_real_verify_env_ambient(self):
        self.assertEqual(
            self.ambient.rc, 0,
            f"{self.verdict.message}\n{quote_child_output(self.ambient.output)}")

    def test_reports_zero_failed_under_the_real_verify_env_ambient(self):
        self.assertTrue(
            CLEAN_SUMMARY_RE.search(self.ambient.output),
            "no anchored `Results: N passed, 0 failed` line in the nested "
            f"output:\n{quote_child_output(self.ambient.output)}")


if __name__ == "__main__":
    unittest.main()
