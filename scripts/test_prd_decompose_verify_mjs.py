#!/usr/bin/env python3
"""
test_prd_decompose_verify_mjs.py — tests for scripts/prd-decompose-verify.mjs driven under node.

Skip-guarded on node being on PATH.  tests/infra/test_prd_decompose_verify.sh
runs this partition together with the other test_prd_decompose_verify*.py files.
"""

import functools
import json
import os
import subprocess
import tempfile
import unittest

from _pdv_test_support import _PDV_MJS


# ---------------------------------------------------------------------------
# Cached `node --input-type=module` runner
# ---------------------------------------------------------------------------


@functools.lru_cache(maxsize=None)
def _run_node_module(source: str):
    """Run a Node ESM harness and return (returncode, stdout, stderr).

    Cached on the source text.  Every .mjs test drives node through a harness
    string that is a pure function of class constants, so the same few sources
    were being re-executed once per assertion: the schema extraction (a pair of
    static object literals) span node ~10 times, and TestMjsBatchDisposition
    re-ran its identical scenario A four times.  Node start-up dominates this
    suite's wall clock; the cache removes the duplication without changing what
    any test asserts.

    Only the RAW output is cached — callers still `json.loads` per call, so no
    test can mutate a structure another test will read.  Safe because the .mjs
    under test cannot change mid-run.
    """
    proc = subprocess.run(
        ["node", "--input-type=module"],
        input=source, capture_output=True, text=True, timeout=60,
    )
    return proc.returncode, proc.stdout, proc.stderr


# ---------------------------------------------------------------------------
# step-09 (RED): Workflow .mjs syntax-validity contract
# ---------------------------------------------------------------------------

_NODE_ON_PATH = bool(__import__("shutil").which("node"))


class TestMjsSyntaxValidity(unittest.TestCase):
    """Tests that prd-decompose-verify.mjs exists and is valid ESM.

    Scoped to syntax validity only — NOT prose/role-name grepping and NOT
    executing the module (execution would hit undefined Workflow globals).
    These tests FAIL until step-10 authors the .mjs file.
    """

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs syntax check")
    def test_mjs_file_exists(self):
        """scripts/prd-decompose-verify.mjs must exist."""
        self.assertTrue(
            os.path.isfile(_PDV_MJS),
            f"scripts/prd-decompose-verify.mjs not found at {_PDV_MJS}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs syntax check")
    def test_mjs_node_check_passes(self):
        """Wrapped-form node --check: export-stripped body wrapped in async function is valid syntax.

        After step-17, the .mjs has a top-level `return` which raw `node --check`
        rejects with SyntaxError: Illegal return statement (top-level return is not
        valid ESM). Validate harness-faithful syntax instead: strip `export const
        meta` → `const meta`, wrap the body in `async function __wf() { ... }`, and
        node --check that wrapped form. This mirrors the Workflow harness which wraps
        the script body in an async function before evaluating it.
        """
        with open(_PDV_MJS) as fh:
            src = fh.read()
        stripped = src.replace("export const meta", "const meta")
        wrapped = f"async function __wf() {{\n{stripped}\n}}"
        with tempfile.NamedTemporaryFile(mode="w", suffix=".mjs", delete=False) as f:
            f.write(wrapped)
            tmp_path = f.name
        try:
            result = subprocess.run(
                ["node", "--check", tmp_path],
                capture_output=True,
                text=True,
            )
            self.assertEqual(
                result.returncode, 0,
                f"node --check returned {result.returncode}; stderr: {result.stderr!r}",
            )
        finally:
            os.unlink(tmp_path)


# ---------------------------------------------------------------------------
# step-14 (RED): Workflow .mjs injected-globals runtime contract
# ---------------------------------------------------------------------------

class TestMjsInjectedGlobalsContract(unittest.TestCase):
    """Execute prd-decompose-verify.mjs under a faithful mock of Workflow's injected
    globals and ONLY those: agent, parallel, pipeline, log, phase, args, budget, workflow.

    The Workflow tool injects exactly this set; tmp_file and shell are NOT injected.
    Against the CURRENT .mjs this FAILS: stage 2 calls writeTempJson/runHarness which
    reference non-injected tmp_file/shell globals, throwing ReferenceError at import-eval.
    Passes after step-15 rewrites the .mjs to use ONLY injected globals.
    """

    _SENTINEL = "WORKFLOW_CONTRACT_OK"

    def _harness_source(self) -> str:
        """Build the Node.js ESM harness script source."""
        mjs_abs = _PDV_MJS.replace("\\", "\\\\")
        return f"""\
// Faithful mock of the Workflow tool's injected globals.
// ONLY globals the Workflow tool injects are set — tmp_file and shell are NOT.

const SENTINEL = "{self._SENTINEL}";
const MJS_PATH = "{mjs_abs}";

// ── mock: agent(prompt, opts) — returns canned shapes based on opts.phase ──────
globalThis.agent = async (prompt, opts = {{}}) => {{
    const phase = (opts.phase || "").toLowerCase();
    if (phase === "enumerate") {{
        return {{
            premises: [{{
                text: "revolute rejects non-axis arg",
                assertion_kind: "rejection",
                fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
                match: {{ exit_code: 1 }},
                capability: "arg-vs-param rejection (mock)",
            }}],
        }};
    }}
    if (phase === "prove") {{
        return {{
            prover: [{{
                capability: "arg-vs-param rejection (mock)",
                probe_kind: "check",
                verdict: "PASS",
                command: ["reify", "check", "tests/prd-gate/fixtures/revolute_silent_accept.ri"],
                exit_code: 1,
                stdout: "",
                stderr: "type mismatch",
            }}],
            adversary: [],
        }};
    }}
    if (phase === "adversary") {{
        return {{ prover: [], adversary: [] }};
    }}
    if (phase === "synthesize") {{
        // Full BatchVerdict shape (task #7257): an evidence-free {{blocks:false}}
        // would now be dispositioned NOT_VERIFIED, so the mock must report a
        // real executed probe for these contract tests to drive a VERIFIED leaf.
        return {{ blocks: false, blocking: [], report: "",
                 malformed: [], fixture_absent: [], executed: 1, total: 1 }};
    }}
    // fallback
    return {{}};
}};

// ── mock: pipeline(items, ...stages) — threads each item through stages in order ─
globalThis.pipeline = async (items, ...stages) => {{
    const results = [];
    for (const item of items) {{
        let val = item;
        for (const stage of stages) {{
            val = await stage(val, item, results.length);
        }}
        results.push(val);
    }}
    return results;
}};

// ── mock: parallel(thunks) — Promise.all of called thunks ────────────────────
globalThis.parallel = async (thunks) => Promise.all(thunks.map(t => t()));

// ── mock: log — no-op ─────────────────────────────────────────────────────────
globalThis.log = (..._a) => {{}};

// ── mock: phase — no-op ───────────────────────────────────────────────────────
globalThis.phase = (..._a) => {{}};

// ── mock: args — a single leaf to drive every phase (Enumerate→Prove‖Adversary→Synthesize)
globalThis.args = [{{ signal: "revolute rejects a non-axis arg (mock leaf)", text: "mock leaf" }}];

// ── mock: budget ──────────────────────────────────────────────────────────────
globalThis.budget = {{ total: null, spent: () => 0, remaining: () => Infinity }};

// ── mock: workflow ────────────────────────────────────────────────────────────
globalThis.workflow = async () => {{}};

// ── execute the .mjs via AsyncFunction (harness-faithful) ──────────────────────
import {{ readFileSync }} from "node:fs";
try {{
    let src = readFileSync(MJS_PATH, "utf8");
    src = src.replace("export const meta", "const meta");
    const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
    await new AsyncFunction(src)();
    console.log(SENTINEL);
}} catch (e) {{
    console.error("IMPORT_FAILED:", e.message);
    process.exit(1);
}}
"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_mjs_runs_under_injected_globals_only(self):
        """prd-decompose-verify.mjs runs to completion under ONLY Workflow-injected globals.

        Fails against the current .mjs (writeTempJson/runHarness reference non-injected
        tmp_file/shell globals → ReferenceError). Passes after step-15 rewrite.
        """
        harness_src = self._harness_source()
        with tempfile.NamedTemporaryFile(mode="w", suffix=".mjs", delete=False) as f:
            f.write(harness_src)
            harness_path = f.name
        try:
            result = subprocess.run(
                ["node", "--input-type=module"],
                input=harness_src,
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertEqual(
                result.returncode, 0,
                f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
            )
            self.assertIn(
                self._SENTINEL, result.stdout,
                f"sentinel not found in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
            )
        finally:
            os.unlink(harness_path)

    _RESULT_MARK = "WF_RESULT_JSON:"

    def _result_capturing_source(self) -> str:
        """Build a Node ESM harness that captures the .mjs body's RETURN VALUE.

        The Workflow harness wraps the script body in an async function and takes
        the result from its top-level `return`. After step-17, the .mjs has a
        native top-level `return`, so stripping `export const meta` → `const meta`
        and wrapping in AsyncFunction is sufficient — no return injection needed.
        """
        globals_setup = self._harness_source().split("// ── execute the .mjs")[0]
        return globals_setup + f"""\
// ── capture the .mjs body's return value ─────────────────────────────────────
import {{ readFileSync }} from "node:fs";
const RESULT_MARK = "{self._RESULT_MARK}";
let src = readFileSync(MJS_PATH, "utf8");
// Strip the ESM `export` so the body is legal inside a function.
src = src.replace("export const meta", "const meta");
// The .mjs now has a native top-level `return` (step-17) — no injection needed.
const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
const body = new AsyncFunction(src);
const result = await body();
console.log(RESULT_MARK + JSON.stringify(result));
"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_mjs_surfaces_aggregate_verdict_shape(self):
        """The .mjs body's return value is the aggregate verdict with the right shape.

        Complements test_mjs_returns_verdict_under_documented_contract: that test
        asserts the return value is NOT undefined; this test additionally asserts
        the shape (blocks/leaf_verdicts/summary), the mock-leaf count, and the
        non-blocking outcome. Uses _result_capturing_source() which strips `export`
        and wraps in AsyncFunction — after step-17, no return injection is needed.
        """
        harness_src = self._result_capturing_source()
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        marker_lines = [ln for ln in result.stdout.splitlines()
                        if ln.startswith(self._RESULT_MARK)]
        self.assertTrue(
            marker_lines,
            f"no result marker in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
        )
        payload = marker_lines[-1][len(self._RESULT_MARK):]
        self.assertNotEqual(
            payload, "undefined",
            "workflow body completion value is undefined — result was dropped "
            "(dead-const regression: IIFE bound to an unreturned variable)",
        )
        verdict = json.loads(payload)
        self.assertIsInstance(verdict, dict,
                              f"aggregate verdict must be an object; got {verdict!r}")
        # Aggregate-verdict shape (the contract β/D4 consumes).
        for key in ("blocks", "leaf_verdicts", "summary"):
            self.assertIn(key, verdict,
                          f"aggregate verdict missing '{key}'; got keys {sorted(verdict)}")
        self.assertIsInstance(verdict["blocks"], bool)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertIsInstance(verdict["summary"], str)
        # The mock drives one leaf through to a non-blocking synthesize verdict.
        self.assertFalse(verdict["blocks"],
                         f"mock leaf must not block; got verdict {verdict!r}")
        self.assertEqual(len(verdict["leaf_verdicts"]), 1,
                         f"one mock leaf → one leaf verdict; got {verdict['leaf_verdicts']!r}")

    # ── step-16 RED / step-17 GREEN ───────────────────────────────────────────

    def _honest_contract_source(self) -> str:
        """Build a Node ESM harness that wraps the .mjs body via AsyncFunction
        with NO source rewriting beyond stripping the ESM `export` keyword.

        The Workflow harness documentation states the script body is wrapped in
        an async function and the result taken from its top-level `return` (every
        Workflow doc example ends `return {...}`). A bare IIFE expression at the
        end of the body evaluates the inner IIFE but the OUTER AsyncFunction still
        has no return statement — it resolves to undefined. This test catches that.
        """
        globals_setup = self._harness_source().split("// ── execute the .mjs")[0]
        honest_section = (
            "// ── honest contract: strip export only, NO return injection ────────────────\n"
            "import { readFileSync } from \"node:fs\";\n"
            "const WF_VERDICT_MARK = \"WF_VERDICT_JSON:\";\n"
            "let src = readFileSync(MJS_PATH, \"utf8\");\n"
            "// ONLY strip the ESM `export` keyword — do NOT inject any `return` statement.\n"
            "src = src.replace(\"export const meta\", \"const meta\");\n"
            "const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;\n"
            "const result = await new AsyncFunction(src)();\n"
            "// null sentinel: undefined (missing top-level return) renders as null.\n"
            "console.log(WF_VERDICT_MARK + JSON.stringify(result ?? null));\n"
        )
        return globals_setup + honest_section

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_mjs_returns_verdict_under_documented_contract(self):
        """Workflow contract: .mjs body, wrapped as AsyncFunction with NO source
        rewriting beyond stripping `export`, must RETURN the aggregate verdict.

        The Workflow tool documentation states the harness wraps the script body in
        an async function and takes the result from its top-level `return` — every
        Workflow doc example ends `return {...}`. This test models that exactly: it
        does NOT inject any `return` statement. If the .mjs body has no top-level
        `return` (bare IIFE expression), AsyncFunction returns undefined and the
        {blocks, leaf_verdicts, summary} verdict is silently dropped.

        Against the CURRENT bare-IIFE .mjs, AsyncFunction over the export-stripped
        body returns undefined (the `await (async function runWorkflow(){...})()`
        is a bare expression statement in the outer function — no return), so this
        test FAILS (RED). Passes after step-17 rewrites the .mjs with a top-level
        `return`.
        """
        harness_src = self._honest_contract_source()
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        MARK = "WF_VERDICT_JSON:"
        marker_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(MARK)]
        self.assertTrue(
            marker_lines,
            f"no verdict marker in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
        )
        payload = marker_lines[-1][len(MARK):]
        self.assertNotEqual(
            payload, "null",
            "workflow body returned null/undefined under documented Workflow contract: "
            "bare IIFE expression doesn't return from the outer AsyncFunction, "
            "silently dropping the {blocks, leaf_verdicts, summary} verdict",
        )
        verdict = json.loads(payload)
        self.assertIsInstance(verdict, dict,
                              f"aggregate verdict must be an object; got {verdict!r}")
        for key in ("blocks", "leaf_verdicts", "summary"):
            self.assertIn(key, verdict,
                          f"aggregate verdict missing key '{key}'; keys: {sorted(verdict)}")
        self.assertIsInstance(verdict["blocks"], bool)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertIsInstance(verdict["summary"], str)
        self.assertFalse(verdict["blocks"],
                         f"mock leaf must not block; verdict: {verdict!r}")
        self.assertEqual(
            len(verdict["leaf_verdicts"]), 1,
            f"one mock leaf → one leaf verdict; got {verdict['leaf_verdicts']!r}",
        )

    # ── task #4960 RED: mega-leaf fan-out regression (end-to-end) ────────────
    #
    # Reuses _harness_source()'s injected-globals mock verbatim but parameterizes
    # globalThis.args and captures globalThis.log, then runs the FULL .mjs body
    # (export-stripped -> AsyncFunction, same as _result_capturing_source) to
    # assert the user-observable fan-out signal: leaf_verdicts.length.

    _LOG_MARK = "WF_LOG_JSON:"

    def _fanout_source(self, args_js_expr: str) -> str:
        """Build a Node ESM harness like _result_capturing_source() but with a
        PARAMETERIZED globalThis.args and a CAPTURING globalThis.log, so a test
        can drive args→leaves normalization end-to-end and observe both the
        returned verdict and any degradation warnings.
        """
        base = self._harness_source()

        fixed_args_line = (
            'globalThis.args = [{ signal: "revolute rejects a non-axis arg (mock leaf)", '
            'text: "mock leaf" }];'
        )
        self.assertIn(fixed_args_line, base,
                      "fixture drift: _harness_source() args mock line changed shape")
        base = base.replace(fixed_args_line, f"globalThis.args = {args_js_expr};")

        noop_log_line = "globalThis.log = (..._a) => {};"
        self.assertIn(noop_log_line, base,
                      "fixture drift: _harness_source() log mock line changed shape")
        base = base.replace(
            noop_log_line,
            "globalThis.__LOG_LINES = [];\n"
            "globalThis.log = (..._a) => { globalThis.__LOG_LINES.push(_a.map(String).join(\" \")); };",
        )

        globals_setup = base.split("// ── execute the .mjs")[0]
        return globals_setup + f"""\
// ── capture the .mjs body's return value + log lines ──────────────────────────
import {{ readFileSync }} from "node:fs";
const RESULT_MARK = "{self._RESULT_MARK}";
const LOG_MARK = "{self._LOG_MARK}";
let src = readFileSync(MJS_PATH, "utf8");
src = src.replace("export const meta", "const meta");
const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
const body = new AsyncFunction(src);
const result = await body();
console.log(RESULT_MARK + JSON.stringify(result));
console.log(LOG_MARK + JSON.stringify(globalThis.__LOG_LINES || []));
"""

    def _run_fanout(self, args_js_expr: str):
        """Run the .mjs body under _fanout_source() and return (verdict, log_lines)."""
        harness_src = self._fanout_source(args_js_expr)
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        result_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(self._RESULT_MARK)]
        log_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(self._LOG_MARK)]
        self.assertTrue(result_lines, f"no result marker in stdout; stdout: {result.stdout!r}")
        self.assertTrue(log_lines, f"no log marker in stdout; stdout: {result.stdout!r}")
        verdict = json.loads(result_lines[-1][len(self._RESULT_MARK):])
        logs = json.loads(log_lines[-1][len(self._LOG_MARK):])
        return verdict, logs

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_stringified_leaf_array_fans_out_per_leaf(self):
        """task #4960: Workflow args arriving JSON-stringified must still fan out
        one leaf per element, not collapse into a single mega-leaf.

        Against the CURRENT .mjs, line 111's inline
        `Array.isArray(args) ? args : (args ? [args] : [])` sees a truthy STRING
        (JSON.stringify of the leaf array), Array.isArray fails, and the whole
        batch becomes ONE mega-leaf: leaf_verdicts.length is 1, not 3. RED until
        normalizeLeaves() is wired in.
        """
        args_expr = 'JSON.stringify(["mock-leaf-0", "mock-leaf-1", "mock-leaf-2"])'
        verdict, _logs = self._run_fanout(args_expr)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertEqual(
            len(verdict["leaf_verdicts"]), 3,
            f"stringified 3-leaf array must fan out to 3 leaf verdicts; got {verdict['leaf_verdicts']!r}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_non_json_string_args_falls_back_with_warning(self):
        """A non-JSON string in args has no recoverable leaf structure: it must
        fall back to a single mega-leaf AND log a loud degradation warning so the
        fan-out loss stays visible (task #4960 — the fix must not fail silently).

        Against the CURRENT .mjs, the single-leaf fallback already happens (a
        random string was never an array), but NO warning is logged anywhere —
        this test's warning assertion is RED until normalizeLeaves() calls warn().
        """
        args_expr = json.dumps("not valid json {[")
        verdict, logs = self._run_fanout(args_expr)
        self.assertEqual(
            len(verdict["leaf_verdicts"]), 1,
            f"non-JSON string must fall back to exactly one leaf; got {verdict['leaf_verdicts']!r}",
        )
        combined_log = " ".join(logs).lower()
        self.assertTrue(
            "mega-leaf" in combined_log or "mega leaf" in combined_log
            or "fan-out" in combined_log or "fan out" in combined_log,
            f"expected a mega-leaf/fan-out degradation warning in captured log; got {logs!r}",
        )


# ---------------------------------------------------------------------------
# _HeadSliceHelperMixin — shared node harness for a single pure helper sliced
# out of prd-decompose-verify.mjs (task #7369 review, SPOT): TestMjsNormalize-
# Leaves and TestMjsDroppedLeafLabels each used to hand-roll the same slice/
# extract/run/parse boilerplate. A subclass supplies only the helper's name,
# any one-time setup JS it needs (default: none), and the `cases` object
# literal to evaluate.
# ---------------------------------------------------------------------------

class _HeadSliceHelperMixin:
    """Extracts ONE named function from everything in the .mjs BEFORE the
    `const _wfResult = await` IIFE anchor (so the IIFE itself never runs), via
    `new Function`, and runs it under node — no injected-globals mock.
    """

    _MARK = "HEAD_SLICE_RESULT:"

    def _helper_name(self) -> str:
        raise NotImplementedError

    def _setup_js(self) -> str:
        """Extra JS emitted after helper-extraction, before `cases` is built."""
        return ""

    def _cases_js(self) -> str:
        raise NotImplementedError

    def _harness_source(self) -> str:
        mjs_abs = _PDV_MJS.replace("\\", "\\\\")
        helper = self._helper_name()
        return f"""\
import {{ readFileSync }} from "node:fs";

const MARK = "{self._MARK}";
const MJS_PATH = "{mjs_abs}";

let src = readFileSync(MJS_PATH, "utf8");
const ANCHOR = "const _wfResult = await";
const anchorIdx = src.indexOf(ANCHOR);
if (anchorIdx === -1) {{
    console.error("ANCHOR_NOT_FOUND: " + ANCHOR);
    process.exit(1);
}}
let head = src.slice(0, anchorIdx);
head = head.replace("export const meta", "const meta");

let {helper};
try {{
    {helper} = new Function(head + "\\nreturn {helper};")();
}} catch (e) {{
    console.error("HELPER_EXTRACT_FAILED: " + e.message);
    process.exit(1);
}}
if (typeof {helper} !== "function") {{
    console.error("HELPER_NOT_A_FUNCTION: " + typeof {helper});
    process.exit(1);
}}

{self._setup_js()}
const cases = {self._cases_js()};

console.log(MARK + JSON.stringify(cases));
"""

    def _run_cases(self) -> dict:
        """Run the harness under node; return the parsed `cases` object."""
        harness_src = self._harness_source()
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        marker_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(self._MARK)]
        self.assertTrue(
            marker_lines,
            f"no result marker in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
        )
        return json.loads(marker_lines[-1][len(self._MARK):])


# ---------------------------------------------------------------------------
# task #4960 (RED): normalizeLeaves() pure-helper contract — mega-leaf trap
# ---------------------------------------------------------------------------

class TestMjsNormalizeLeaves(unittest.TestCase, _HeadSliceHelperMixin):
    """Pure-unit tests for the normalizeLeaves(rawArgs, warn) helper, source-
    sliced directly out of prd-decompose-verify.mjs via _HeadSliceHelperMixin
    — no injected-globals mock, no IIFE execution.

    Guards the mega-leaf trap (task #4960): when Workflow args arrive
    JSON-stringified, `Array.isArray(args)` is false and the whole stringified
    batch collapses into ONE leaf. normalizeLeaves must detect a JSON-
    stringified array and restore per-leaf fan-out, while leaving every
    non-string input byte-for-byte unchanged (real array / single object /
    undefined / null).

    FAILS until normalizeLeaves is added to the .mjs: the head-slice eval
    throws ReferenceError('normalizeLeaves is not defined'), the Node harness
    exits non-zero, and the test fails on the returncode assertion.
    """

    def _helper_name(self) -> str:
        return "normalizeLeaves"

    def _setup_js(self) -> str:
        return """\
function run(rawArgs) {
    const warnCalls = [];
    const warn = (...a) => { warnCalls.push(a.map(String).join(" ")); };
    const result = normalizeLeaves(rawArgs, warn);
    return {
        length: Array.isArray(result) ? result.length : null,
        isArray: Array.isArray(result),
        warnCalls,
    };
}

const L = [{ signal: "leaf-1" }, { signal: "leaf-2" }, { signal: "leaf-3" }];
"""

    def _cases_js(self) -> str:
        return """{
    stringified_array: run(JSON.stringify(L)),
    real_array_passthrough: run(L),
    non_json_string: run("not json {["),
    json_non_array_object: run(JSON.stringify({ a: 1 })),
    undefined_input: run(undefined),
    empty_string: run(""),
    single_object_leaf: run({ signal: "solo" }),
}"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs helper contract test")
    def test_normalize_leaves_pure_helper_contract(self):
        cases = self._run_cases()

        # (a) stringified array -> real per-leaf fan-out, no degradation warning.
        self.assertEqual(
            cases["stringified_array"]["length"], 3,
            f"JSON-stringified 3-leaf array must fan out to 3 leaves; got {cases['stringified_array']!r}",
        )
        self.assertEqual(
            cases["stringified_array"]["warnCalls"], [],
            "a well-formed stringified leaf array must not log a degradation warning",
        )

        # (b) real array passthrough — unchanged behavior, no warning.
        self.assertEqual(
            cases["real_array_passthrough"]["length"], 3,
            f"real array must pass through unchanged; got {cases['real_array_passthrough']!r}",
        )
        self.assertEqual(cases["real_array_passthrough"]["warnCalls"], [])

        # (c) non-JSON string -> single mega-leaf fallback + loud warning.
        self.assertEqual(
            cases["non_json_string"]["length"], 1,
            f"non-JSON string must fall back to one mega-leaf; got {cases['non_json_string']!r}",
        )
        self.assertTrue(
            cases["non_json_string"]["warnCalls"],
            "non-JSON string fallback must log a degradation warning",
        )
        warn_text = " ".join(cases["non_json_string"]["warnCalls"]).lower()
        self.assertTrue(
            "mega-leaf" in warn_text or "mega leaf" in warn_text
            or "fan-out" in warn_text or "fan out" in warn_text,
            f"warning must name the mega-leaf/fan-out degradation; got {cases['non_json_string']['warnCalls']!r}",
        )

        # edge: JSON string parsing to a non-array object -> single mega-leaf + warning.
        self.assertEqual(
            cases["json_non_array_object"]["length"], 1,
            f"non-array JSON must fall back to one mega-leaf; got {cases['json_non_array_object']!r}",
        )
        self.assertTrue(
            cases["json_non_array_object"]["warnCalls"],
            "non-array JSON parse result must log a degradation warning",
        )

        # edge: undefined / empty string -> zero leaves (unchanged pre-fix behavior).
        self.assertEqual(cases["undefined_input"]["length"], 0)
        self.assertEqual(cases["undefined_input"]["warnCalls"], [])
        self.assertEqual(cases["empty_string"]["length"], 0)

        # edge: single object leaf (non-string, non-array) -> one leaf, unchanged.
        self.assertEqual(cases["single_object_leaf"]["length"], 1)
        self.assertEqual(cases["single_object_leaf"]["warnCalls"], [])


# ---------------------------------------------------------------------------
# task #7369 (RED before the fix): droppedLeafLabels() names the actual null
# positions, not an arithmetic guess at where they are
# ---------------------------------------------------------------------------

class TestMjsDroppedLeafLabels(unittest.TestCase, _HeadSliceHelperMixin):
    """Pure-unit tests for the droppedLeafLabels(leaves, leaf_verdicts) helper,
    source-sliced directly out of prd-decompose-verify.mjs via
    _HeadSliceHelperMixin — same technique as TestMjsNormalizeLeaves above.

    Guards the defect described in task #7369: the pre-fix expression derived
    each dropped leaf's label from `j >= (leaves.length - dropped - i)`, an
    arithmetic guess at a TAIL position that bears no relation to where the
    nulls actually are. Measured on this branch before the fix (node
    simulation of the exact pre-fix expression):
        5 leaves, dropped at indices 0 and 4 -> ["<dropped-leaf:4>","<dropped-leaf:4>"]
            (index 0 never named; index 4 named twice)
        5 leaves, single drop at index 1     -> ["<dropped-leaf:?>"]
            (index is knowable; reported unknown)
    The fix derives each label directly from the null's own position in
    leaf_verdicts, so it is correct regardless of contiguity — the tail-only
    case (single drop at the last index) passed even before the fix and would
    not have caught this, hence the non-contiguous cases below.

    Also covers (task #7369 review): the object-leaf label branches (signal /
    text / neither-fallback), and the null-safety fix for leafLabelFor — a
    leaf element that is itself null/undefined, or a leaves array shorter than
    leaf_verdicts, must label via the index fallback rather than throwing.

    And (task #7369 review round 2): the pipeline()-contract defense. Every
    case above trusts that leaf_verdicts is exactly leaves.length long — the
    contract the real pipeline() runtime is documented to uphold, but which
    this repo cannot pin. If leaf_verdicts ever comes back SHORTER than
    leaves (a hypothetical compacting runtime), the leaves past its end have
    no hole to find; droppedLeafLabels must name that gap explicitly rather
    than silently under-counting.
    """

    def _helper_name(self) -> str:
        return "droppedLeafLabels"

    def _setup_js(self) -> str:
        return """\
const LEAVES = ["leaf0", "leaf1", "leaf2", "leaf3", "leaf4"];
const OBJECT_LEAVES = [{ signal: "sig-a" }, { text: "txt-b" }, {}];
"""

    def _cases_js(self) -> str:
        return """{
    // non-contiguous: dropped at 0 and 4 (the case the pre-fix arithmetic got wrong twice over)
    non_contiguous_head_and_tail: droppedLeafLabels(
        LEAVES, [null, {blocks: false}, {blocks: false}, {blocks: false}, null]),
    // knowable single mid-array drop (pre-fix reported "?" here)
    single_drop_mid_array: droppedLeafLabels(
        LEAVES, [{blocks: false}, null, {blocks: false}, {blocks: false}, {blocks: false}]),
    // tail-contiguous single drop — passed even before the fix; regression guard
    single_drop_tail: droppedLeafLabels(
        LEAVES, [{blocks: false}, {blocks: false}, {blocks: false}, {blocks: false}, null]),
    // no drops at all
    no_drops: droppedLeafLabels(
        LEAVES, [{blocks: false}, {blocks: false}, {blocks: false}, {blocks: false}, {blocks: false}]),
    // object leaves: signal / text / neither-fallback branches of leafLabelFor
    object_leaf_signal: droppedLeafLabels(
        OBJECT_LEAVES, [null, {blocks: false}, {blocks: false}]),
    object_leaf_text: droppedLeafLabels(
        OBJECT_LEAVES, [{blocks: false}, null, {blocks: false}]),
    object_leaf_neither: droppedLeafLabels(
        OBJECT_LEAVES, [{blocks: false}, {blocks: false}, null]),
    // null-safety: a leaf element that is itself null/undefined must not throw
    null_leaf_element: droppedLeafLabels(
        [null, "a"], [null, {blocks: false}]),
    short_leaves_array: droppedLeafLabels(
        ["a"], [null, null]),
    // contract violation: leaf_verdicts came back SHORTER than leaves (a
    // hypothetical compacting pipeline()) — the missing tail must be named,
    // not silently invisible.
    contract_violation_shrunk_array: droppedLeafLabels(
        LEAVES, [{blocks: false}, {blocks: false}, {blocks: false}]),
    // shrinkage combined with a real hole inside the surviving prefix — both
    // must be reported.
    contract_violation_with_real_drop: droppedLeafLabels(
        LEAVES, [null, {blocks: false}, {blocks: false}]),
}"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs helper contract test")
    def test_dropped_leaf_labels_pure_helper_contract(self):
        cases = self._run_cases()

        # The defect: index 0 never named, index 4 named twice.
        self.assertEqual(
            cases["non_contiguous_head_and_tail"],
            ["<dropped-leaf:0:leaf0>", "<dropped-leaf:4:leaf4>"],
            "each dropped index must be named exactly once, at its own position",
        )

        # The defect: a knowable mid-array index was reported as "?".
        self.assertEqual(
            cases["single_drop_mid_array"], ["<dropped-leaf:1:leaf1>"],
            "a knowable dropped index must never fall back to '?'",
        )

        # Regression guard: the one case the pre-fix arithmetic got right by luck.
        self.assertEqual(cases["single_drop_tail"], ["<dropped-leaf:4:leaf4>"])

        # No drops -> no labels, and the count always matches the null count.
        self.assertEqual(cases["no_drops"], [])
        # Emitted label count must equal the number of nulls in each fixture.
        self.assertEqual(len(cases["non_contiguous_head_and_tail"]), 2)
        self.assertEqual(len(cases["single_drop_mid_array"]), 1)
        self.assertEqual(len(cases["single_drop_tail"]), 1)
        self.assertEqual(len(cases["no_drops"]), 0)

        # Object-leaf branches: signal, then text, then the leaf-${idx} fallback.
        self.assertEqual(cases["object_leaf_signal"], ["<dropped-leaf:0:sig-a>"])
        self.assertEqual(cases["object_leaf_text"], ["<dropped-leaf:1:txt-b>"])
        self.assertEqual(cases["object_leaf_neither"], ["<dropped-leaf:2:leaf-2>"])

        # leafLabelFor must be total: a null leaf element, or a leaves array
        # shorter than leaf_verdicts (so leaves[j] is undefined), must label
        # via the index fallback rather than throwing.
        self.assertEqual(cases["null_leaf_element"], ["<dropped-leaf:0:leaf-0>"])
        self.assertEqual(
            cases["short_leaves_array"],
            ["<dropped-leaf:0:a>", "<dropped-leaf:1:leaf-1>"],
        )

        # Contract violation (task #7369 review round 2): leaf_verdicts came
        # back shorter than leaves — 5 leaves, only 3 verdict slots, none of
        # them null. The 2 missing leaves must still surface as a named,
        # counted hole instead of vanishing.
        self.assertEqual(
            cases["contract_violation_shrunk_array"],
            ["<dropped-leaf:contract-violation:2>"],
            "a leaf_verdicts array shorter than leaves must name the gap, not vanish it",
        )
        # Shrinkage stacks with a real hole in the surviving prefix: both the
        # named null at index 0 and the contract-violation gap must appear.
        self.assertEqual(
            cases["contract_violation_with_real_drop"],
            ["<dropped-leaf:0:leaf0>", "<dropped-leaf:contract-violation:2>"],
        )


# ---------------------------------------------------------------------------
# task #7257 step-09 (RED): RESULTS_SCHEMA must constrain the record shape
# ---------------------------------------------------------------------------

class TestMjsResultsSchema(unittest.TestCase):
    """The agent-output schema is the FIRST line of defence for ARM 1.

    RESULTS_SCHEMA today declares `prover`/`adversary` as
    `{type:"array", items:{type:"object"}}` — no required keys, no verdict
    enum, no type on `command`.  Consequences measured on this branch:

      - a PREMISE record validates as a RESULT record;
      - a record with no `command`/`exit_code` at all validates, which is
        exactly the unexecuted promise the Python evidence gate now catches;
      - `"command": "target/release/reify eval f.ri"` (a STRING) validates,
        which is the character-explosion source.

    The Python gate is the second line of defence and stays.  This schema kills
    the malformed shapes at source so an agent cannot emit them at all.

    Source-sliced out of the .mjs the same way TestMjsNormalizeLeaves does:
    everything BEFORE the `const _wfResult = await` IIFE anchor, evaluated via
    `new Function` — no injected-globals mock, no IIFE execution.

    GREEN in task #7257 step-10.
    """

    _MARK = "SCHEMAS_RESULT:"
    _REQUIRED_KEYS = {"capability", "verdict", "command", "exit_code"}
    _VERDICT_ENUM = ["PASS", "FAIL", "UNPROVABLE", "HARNESS_ERROR"]

    def _harness_source(self) -> str:
        mjs_abs = _PDV_MJS.replace("\\", "\\\\")
        return f"""\
import {{ readFileSync }} from "node:fs";

const MARK = "{self._MARK}";
const MJS_PATH = "{mjs_abs}";

let src = readFileSync(MJS_PATH, "utf8");
const ANCHOR = "const _wfResult = await";
const anchorIdx = src.indexOf(ANCHOR);
if (anchorIdx === -1) {{
    console.error("ANCHOR_NOT_FOUND: " + ANCHOR);
    process.exit(1);
}}
let head = src.slice(0, anchorIdx);
head = head.replace("export const meta", "const meta");

let schemas;
try {{
    schemas = new Function(head + "\\nreturn {{ RESULTS_SCHEMA, VERDICT_SCHEMA }};")();
}} catch (e) {{
    console.error("SCHEMA_EXTRACT_FAILED: " + e.message);
    process.exit(1);
}}
console.log(MARK + JSON.stringify(schemas));
"""

    def _schemas(self) -> dict:
        """Run the extraction harness and return {RESULTS_SCHEMA, VERDICT_SCHEMA}.

        The node run is cached (see _run_node_module) — the schemas are static
        literals, so re-spawning node per assertion bought nothing.
        """
        rc, out, err = _run_node_module(self._harness_source())
        self.assertEqual(rc, 0, f"node exited {rc}; stderr: {err!r}")
        lines = [ln for ln in out.splitlines() if ln.startswith(self._MARK)]
        self.assertTrue(lines, f"no schema marker in stdout; stdout: {out!r}")
        return json.loads(lines[-1][len(self._MARK):])

    def _record_items(self, role: str) -> dict:
        """The `items` sub-schema constraining one α result record for `role`."""
        results = self._schemas()["RESULTS_SCHEMA"]
        self.assertIn(role, results["properties"],
                      f"RESULTS_SCHEMA has no {role!r} property")
        prop = results["properties"][role]
        self.assertIn("items", prop, f"{role} declares no `items` record constraint")
        return prop["items"]

    # ── (1)(2) both roles constrain the record's required keys ───────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_prover_items_require_the_evidence_keys(self):
        """A prover record must declare capability, verdict, command AND exit_code."""
        items = self._record_items("prover")
        required = set(items.get("required", []))
        self.assertTrue(
            self._REQUIRED_KEYS.issubset(required),
            f"prover items.required is missing {self._REQUIRED_KEYS - required}; "
            f"got {sorted(required)}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_adversary_items_require_the_evidence_keys(self):
        """The Adversary is constrained too — not just the Prover.

        The Adversary is the role that can only ADD blocking signals, so an
        unconstrained adversary record is the cheapest route to a vacuous block.
        """
        items = self._record_items("adversary")
        required = set(items.get("required", []))
        self.assertTrue(
            self._REQUIRED_KEYS.issubset(required),
            f"adversary items.required is missing {self._REQUIRED_KEYS - required}; "
            f"got {sorted(required)}",
        )

    # ── (3) the verdict vocabulary is closed ─────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_verdict_is_a_closed_enum(self):
        """PRD §6 decision 3 fixes the verdict vocabulary; the schema pins it."""
        for role in ("prover", "adversary"):
            with self.subTest(role=role):
                props = self._record_items(role).get("properties", {})
                self.assertIn("verdict", props, f"{role} items declares no verdict")
                self.assertEqual(props["verdict"].get("enum"), self._VERDICT_ENUM)

    # ── (4) the constraint that kills the string-command shape at source ─────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_command_is_an_array_of_strings(self):
        """`command` must be argv tokens, never a ready-to-paste shell string."""
        for role in ("prover", "adversary"):
            with self.subTest(role=role):
                props = self._record_items(role).get("properties", {})
                self.assertIn("command", props, f"{role} items declares no command")
                self.assertEqual(
                    props["command"],
                    {"type": "array", "items": {"type": "string"}},
                    f"{role} command must be an array of strings; got {props['command']!r}",
                )

    # ── (5) exit_code is an integer, and null is not an accepted type ────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_exit_code_is_an_integer_and_not_nullable(self):
        """A null exit_code is the unexecuted-promise shape; the schema rejects it."""
        for role in ("prover", "adversary"):
            with self.subTest(role=role):
                props = self._record_items(role).get("properties", {})
                self.assertIn("exit_code", props, f"{role} items declares no exit_code")
                declared = props["exit_code"].get("type")
                self.assertEqual(declared, "integer",
                                 f"{role} exit_code.type must be 'integer'; got {declared!r}")
                self.assertNotIn(
                    "null", declared if isinstance(declared, list) else [declared],
                    f"{role} exit_code must not accept null",
                )

    # ── (6) VERDICT_SCHEMA declares, but does not require, the new fields ────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_verdict_schema_declares_the_new_batchverdict_fields(self):
        """The harness now emits malformed/fixture_absent/executed/total."""
        props = self._schemas()["VERDICT_SCHEMA"].get("properties", {})
        for key in ("malformed", "fixture_absent", "executed", "total"):
            self.assertIn(key, props,
                          f"VERDICT_SCHEMA declares no {key!r}; got {sorted(props)}")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_verdict_schema_requires_the_executed_counters(self):
        """`executed`/`total` are REQUIRED; `malformed`/`fixture_absent` are not.

        These were left optional on the reasoning that requiring them would
        hard-fail against an older harness build — a skew that cannot occur, as
        the Synthesize agent shells the harness out of the SAME checkout as the
        .mjs.  What a loose schema DID admit is a Synthesize agent that drops
        the counters despite the prompt: `executed ?? 0` then dispositions every
        leaf NOT_VERIFIED and the batch INCOMPLETE, indistinguishable from
        genuine incompleteness.  The schema is the enforcement point that makes
        the agent retry instead of laundering a relay bug into a verdict.

        `malformed`/`fixture_absent` stay optional — absent and empty mean the
        same thing for a list, and neither carries disposition weight of its own.
        """
        schema = self._schemas()["VERDICT_SCHEMA"]
        required = schema.get("required", [])
        self.assertEqual(required,
                         ["blocks", "blocking", "report", "executed", "total"])
        self.assertNotIn("malformed", required)
        self.assertNotIn("fixture_absent", required)
        self.assertEqual(schema["properties"]["executed"].get("type"), "integer")
        self.assertEqual(schema["properties"]["total"].get("type"), "integer")


# ---------------------------------------------------------------------------
# task #7257 steps 11/13: parameterized .mjs scenario harness
# ---------------------------------------------------------------------------

_MJS_RESULT_MARK = "SCENARIO_RESULT:"
_MJS_PHASES_MARK = "SCENARIO_PHASES:"
_MJS_LOGS_MARK = "SCENARIO_LOGS:"
_MJS_PIPELINE_ERRORS_MARK = "SCENARIO_PIPELINE_ERRORS:"


def _mjs_scenario_source(leaves_js: str, responses_js: str) -> str:
    """Build a Node ESM harness that runs the FULL .mjs body under a mock of
    Workflow's injected globals and ONLY those, with a PARAMETERIZED agent.

    Args:
        leaves_js:    a JS expression for globalThis.args (the leaf array).
        responses_js: a JS expression evaluating to an object mapping a
                      lower-cased phase name to either a value or a
                      (prompt, opts) => value function.

    The harness records every phase the .mjs actually invokes, so a test can
    assert on stages that were SKIPPED as well as on the returned verdict.
    """
    mjs_abs = _PDV_MJS.replace("\\", "\\\\")
    return f"""\
import {{ readFileSync }} from "node:fs";

const MJS_PATH = "{mjs_abs}";
const RESULT_MARK = "{_MJS_RESULT_MARK}";
const PHASES_MARK = "{_MJS_PHASES_MARK}";
const LOGS_MARK = "{_MJS_LOGS_MARK}";
const PIPELINE_ERRORS_MARK = "{_MJS_PIPELINE_ERRORS_MARK}";

// ── mock: agent(prompt, opts) — parameterized, and records each phase ────────
globalThis.__PHASES = [];
const RESPONSES = {responses_js};
globalThis.agent = async (prompt, opts = {{}}) => {{
    const phase = (opts.phase || "").toLowerCase();
    globalThis.__PHASES.push(phase);
    const r = RESPONSES[phase];
    if (typeof r === "function") return r(prompt, opts);
    if (r !== undefined) return r;
    return {{}};
}};

// ── mock: pipeline(items, ...stages) — threads each item through in order,
// dropping to null (at the item's own index) if any stage throws — mirrors
// the documented real-pipeline contract the aggregation code relies on: the
// workflow-authoring reference states "A stage that throws drops that item
// to null and skips its remaining stages", implemented there via
// Promise.allSettled + an index-preserving map. Record the caught error the
// same way the real runtime does (`pipeline[<index>] failed: <message>`)
// instead of discarding it — otherwise a genuine .mjs stage defect (a
// TypeError from a renamed field, a bad JSON.stringify arg) is invisible in
// these scenario tests, indistinguishable from an intentionally-thrown mock.
//
// This is a HARNESS self-check, not SUT behavior — the real .mjs never calls
// this catch block, only this mock does — so it is recorded into its own
// __PIPELINE_ERRORS channel rather than __LOG_LINES (task #7369 review round
// 2). __LOG_LINES stays a faithful, harness-untouched record of exactly what
// the .mjs itself passed to log(), so a future assertion over it (an exact
// line count, an "the .mjs logged nothing on this path" check) can't be
// tripped up by text this harness injected.
globalThis.pipeline = async (items, ...stages) => {{
    const results = [];
    for (const item of items) {{
        let val = item;
        try {{
            for (const stage of stages) {{
                val = await stage(val, item, results.length);
            }}
        }} catch (e) {{
            globalThis.__PIPELINE_ERRORS.push(`pipeline[${{results.length}}] failed: ` + e.message);
            val = null;
        }}
        results.push(val);
    }}
    return results;
}};

globalThis.parallel = async (thunks) => Promise.all(thunks.map(t => t()));
globalThis.__LOG_LINES = [];
globalThis.log = (..._a) => {{ globalThis.__LOG_LINES.push(_a.map(String).join(" ")); }};
globalThis.__PIPELINE_ERRORS = [];
globalThis.phase = (..._a) => {{}};
globalThis.args = {leaves_js};
globalThis.budget = {{ total: null, spent: () => 0, remaining: () => Infinity }};
globalThis.workflow = async () => {{}};

// ── execute the .mjs body and capture its top-level return ──────────────────
let src = readFileSync(MJS_PATH, "utf8");
src = src.replace("export const meta", "const meta");
const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
const result = await new AsyncFunction(src)();
console.log(RESULT_MARK + JSON.stringify(result));
console.log(PHASES_MARK + JSON.stringify(globalThis.__PHASES));
console.log(LOGS_MARK + JSON.stringify(globalThis.__LOG_LINES));
console.log(PIPELINE_ERRORS_MARK + JSON.stringify(globalThis.__PIPELINE_ERRORS));
"""


class _MjsScenarioMixin:
    """Shared runner for the parameterized .mjs scenario harness."""

    # A leaf that enumerates one premise, probes it, and synthesizes a clean,
    # EVIDENCE-BACKED verified verdict — the "normal path" control.
    VERIFIED_RESPONSES = """{
    enumerate: { premises: [{
        text: "revolute rejects non-axis arg",
        assertion_kind: "rejection",
        fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
        match: { exit_code: 1 },
        capability: "arg-vs-param rejection (mock)",
    }] },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check",
        verdict: "PASS",
        command: ["reify", "check", "f.ri"],
        exit_code: 1,
        stdout: "",
        stderr: "type mismatch",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [], report: "",
                  malformed: [], fixture_absent: [], executed: 1, total: 1 },
}"""

    # A leaf whose Enumerator returns nothing at all.
    UNENUMERATED_RESPONSES = """{
    enumerate: { premises: [] },
    prove: { prover: [], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [], report: "",
                  malformed: [], fixture_absent: [], executed: 0, total: 0 },
}"""

    def _run_scenario(self, leaves_js: str, responses_js: str):
        """Run the .mjs under the scenario harness; return (verdict, phases).

        Cached per (leaves_js, responses_js) via _run_node_module: a scenario is
        deterministic, so a class asserting five things about one scenario runs
        it once, not five times.
        """
        rc, out, err = _run_node_module(_mjs_scenario_source(leaves_js, responses_js))
        self.assertEqual(
            rc, 0, f"node exited {rc}; stderr: {err!r}; stdout: {out!r}")
        res_lines = [ln for ln in out.splitlines()
                     if ln.startswith(_MJS_RESULT_MARK)]
        ph_lines = [ln for ln in out.splitlines()
                    if ln.startswith(_MJS_PHASES_MARK)]
        self.assertTrue(res_lines, f"no result marker; stdout: {out!r}")
        self.assertTrue(ph_lines, f"no phases marker; stdout: {out!r}")
        verdict = json.loads(res_lines[-1][len(_MJS_RESULT_MARK):])
        phases = json.loads(ph_lines[-1][len(_MJS_PHASES_MARK):])
        return verdict, phases

    def _run_scenario_logs(self, leaves_js: str, responses_js: str):
        """Return the captured globalThis.log() lines for a scenario.

        Kept separate from _run_scenario rather than widening its (verdict,
        phases) return tuple — that tuple is unpacked at ~15 call sites across
        this file that have no use for log lines. _run_node_module is cached
        on the exact generated source (see its docstring), so calling this
        with the same (leaves_js, responses_js) already passed to
        _run_scenario in the same test costs no extra node spawn.
        """
        rc, out, err = _run_node_module(_mjs_scenario_source(leaves_js, responses_js))
        self.assertEqual(
            rc, 0, f"node exited {rc}; stderr: {err!r}; stdout: {out!r}")
        log_lines = [ln for ln in out.splitlines()
                     if ln.startswith(_MJS_LOGS_MARK)]
        self.assertTrue(log_lines, f"no logs marker; stdout: {out!r}")
        return json.loads(log_lines[-1][len(_MJS_LOGS_MARK):])

    def _run_scenario_pipeline_errors(self, leaves_js: str, responses_js: str):
        """Return the harness mock pipeline()'s own caught-error records.

        This is a HARNESS self-check channel, not SUT output (task #7369
        review round 2): it reads __PIPELINE_ERRORS, which only the scenario
        mock's catch block writes to, never the .mjs itself — kept separate
        from _run_scenario_logs's __LOG_LINES so that channel stays a faithful
        record of exactly what prd-decompose-verify.mjs passed to log().
        Cached the same way as _run_scenario_logs (see its docstring).
        """
        rc, out, err = _run_node_module(_mjs_scenario_source(leaves_js, responses_js))
        self.assertEqual(
            rc, 0, f"node exited {rc}; stderr: {err!r}; stdout: {out!r}")
        error_lines = [ln for ln in out.splitlines()
                       if ln.startswith(_MJS_PIPELINE_ERRORS_MARK)]
        self.assertTrue(error_lines, f"no pipeline-errors marker; stdout: {out!r}")
        return json.loads(error_lines[-1][len(_MJS_PIPELINE_ERRORS_MARK):])


# ---------------------------------------------------------------------------
# task #7257 step-11 (RED): a zero-premise leaf is not a verified leaf (ARM 2)
# ---------------------------------------------------------------------------

class TestMjsUnenumeratedLeaf(unittest.TestCase, _MjsScenarioMixin):
    """A leaf whose Enumerator returned zero premises was never probed.

    Measured on this branch BEFORE the fix, driving one zero-premise leaf:
        leaf_verdicts[0] == {leafLabel, blocks: false, blocking: [], report: ""}
        summary          == "γ PASS — all 1 leaf(ves) verified"
        phases           == ["enumerate", "synthesize"]

    That is byte-identical in shape to a genuinely verified leaf, so the ONE
    signal that matters — "nothing was checked here" — is unrecoverable from
    the return value.  It also still burned a Synthesize agent call to
    synthesize an empty record set (α over {} is vacuously non-blocking).

    GREEN in task #7257 step-12.
    """

    _LABEL = "zero-premise leaf (delta)"
    _LEAF_JS = '[{ signal: "zero-premise leaf (delta)" }]'

    def _unenumerated(self):
        return self._run_scenario(self._LEAF_JS, self.UNENUMERATED_RESPONSES)

    # ── (1)(2) the per-leaf disposition ──────────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_leaf_verdict_carries_unenumerated_disposition(self):
        """The leaf verdict says, in one field, that nothing was enumerated."""
        verdict, _ = self._unenumerated()
        self.assertEqual(len(verdict["leaf_verdicts"]), 1)
        leaf = verdict["leaf_verdicts"][0]
        self.assertEqual(
            leaf.get("disposition"), "UNENUMERATED",
            f"zero-premise leaf must be dispositioned UNENUMERATED; got {leaf!r}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_unenumerated_differs_from_a_verified_leaf(self):
        """The whole point: the two outcomes must be distinguishable."""
        unenum, _ = self._unenumerated()
        verified, _ = self._run_scenario(
            '[{ signal: "normally verified leaf" }]', self.VERIFIED_RESPONSES)

        unenum_leaf = unenum["leaf_verdicts"][0]
        verified_leaf = verified["leaf_verdicts"][0]

        self.assertEqual(verified_leaf.get("disposition"), "VERIFIED",
                         f"control leaf must be VERIFIED; got {verified_leaf!r}")
        self.assertNotEqual(
            unenum_leaf.get("disposition"), verified_leaf.get("disposition"),
            "a never-probed leaf must not share a disposition with a verified one",
        )
        # Both are non-blocking — which is exactly why `blocks` alone is not enough.
        self.assertFalse(unenum_leaf["blocks"])
        self.assertFalse(verified_leaf["blocks"])

    # ── (3)(4)(5) the batch-level signal ─────────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_batch_disposition_is_not_pass(self):
        """A batch that probed nothing did not pass."""
        verdict, _ = self._unenumerated()
        self.assertIn("disposition", verdict,
                      f"batch verdict has no disposition; keys {sorted(verdict)}")
        self.assertNotEqual(verdict["disposition"], "PASS")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_unenumerated_leaf_label_is_listed_at_top_level(self):
        """The caller can name the never-probed leaf without opening journal.jsonl."""
        verdict, _ = self._unenumerated()
        self.assertIn("unenumerated_leaves", verdict,
                      f"batch verdict has no unenumerated_leaves; keys {sorted(verdict)}")
        self.assertEqual(verdict["unenumerated_leaves"], [self._LABEL])

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_summary_names_the_leaf_and_the_probed_count(self):
        """The one-line summary must not read as a pass."""
        verdict, _ = self._unenumerated()
        summary = verdict["summary"]
        self.assertIn(self._LABEL, summary,
                      f"summary must name the never-probed leaf; got {summary!r}")
        self.assertIn("0 of 1", summary,
                      f"summary must state the probed count; got {summary!r}")
        self.assertNotIn("PASS", summary,
                         f"a never-probed batch must not summarize as PASS; got {summary!r}")

    # ── (6) the stage-3 short-circuit ────────────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_synthesize_agent_is_not_called_for_a_zero_premise_leaf(self):
        """There is nothing to synthesize — do not pay an agent to say so.

        α over an empty record set is vacuously non-blocking, so the call could
        only ever return a clean verdict.  Skipping it is both cheaper and one
        less way to launder 'nothing ran' into 'nothing failed'.
        """
        _, phases = self._unenumerated()
        self.assertNotIn(
            "synthesize", phases,
            f"stage 3 must short-circuit on an unenumerated leaf; phases {phases!r}",
        )
        self.assertIn("enumerate", phases,
                      f"the Enumerator must still run; phases {phases!r}")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_verified_control_leaf_still_calls_synthesize(self):
        """The short-circuit is scoped to the empty case — regression guard."""
        _, phases = self._run_scenario(
            '[{ signal: "normally verified leaf" }]', self.VERIFIED_RESPONSES)
        self.assertIn("synthesize", phases,
                      f"the normal path must still synthesize; phases {phases!r}")


# ---------------------------------------------------------------------------
# task #7257 step-13 (RED): the batch return must surface the probed count
# ---------------------------------------------------------------------------

class TestMjsBatchDisposition(unittest.TestCase, _MjsScenarioMixin):
    """A session must be able to answer "how many leaves were actually probed?"
    from the workflow's return value, without opening journal.jsonl.

    Today the aggregate carries only blocks / leaf_verdicts / summary, so a
    batch in which NO leaf was ever probed is reported as
    "γ PASS — all N leaf(ves) verified" — the ARM 2 failure.  A third
    disposition is needed between BLOCKS and PASS: INCOMPLETE, meaning nothing
    was falsified but nothing was verified either.

    GREEN in task #7257 step-14.
    """

    # ── scenario A: one verified leaf + one zero-premise leaf ────────────────

    _A_LEAVES = '[{ signal: "verified leaf (alpha)" }, { signal: "zero-premise leaf (beta)" }]'
    _A_RESPONSES = """{
    enumerate: (prompt) => prompt.includes("zero-premise")
        ? { premises: [] }
        : { premises: [{
              text: "revolute rejects non-axis arg",
              assertion_kind: "rejection",
              fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
              match: { exit_code: 1 },
              capability: "arg-vs-param rejection (mock)",
          }] },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check", verdict: "PASS",
        command: ["reify", "check", "f.ri"], exit_code: 1,
        stdout: "", stderr: "type mismatch",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [], report: "",
                  malformed: [], fixture_absent: [], executed: 1, total: 1 },
}"""

    # ── scenario B: two normally verified leaves ─────────────────────────────

    _B_LEAVES = '[{ signal: "verified leaf (alpha)" }, { signal: "verified leaf (gamma)" }]'

    # ── scenario C: a leaf that probed nothing because every record was malformed ─

    _C_LEAVES = '[{ signal: "malformed-records leaf (epsilon)" }]'
    _C_RESPONSES = """{
    enumerate: { premises: [{
        text: "revolute rejects non-axis arg",
        assertion_kind: "rejection",
        fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
        match: { exit_code: 1 },
        capability: "arg-vs-param rejection (mock)",
    }] },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check", verdict: "FAIL",
        command: [], exit_code: -1, stdout: "", stderr: "",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [],
                  report: "MALFORMED (no executed-probe evidence ...)",
                  malformed: ["arg-vs-param rejection (mock)"],
                  fixture_absent: [], executed: 0, total: 1 },
}"""

    # ── scenario D: the Synthesize agent returns null (agent death) ─────────
    #
    # The fail-closed fallback fires: the leaf BLOCKS with no harness output at
    # all.  Nothing was probed, so `leaves_probed` must stay 0 — deriving it
    # from the disposition counted this leaf as probed, overstating coverage on
    # exactly the failure path a reader consults the counter to check.

    _D_LEAVES = '[{ signal: "null-synthesize leaf (zeta)" }]'
    _D_RESPONSES = """{
    enumerate: { premises: [{
        text: "revolute rejects non-axis arg",
        assertion_kind: "rejection",
        fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
        match: { exit_code: 1 },
        capability: "arg-vs-param rejection (mock)",
    }] },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check", verdict: "PASS",
        command: ["reify", "check", "f.ri"], exit_code: 1,
        stdout: "", stderr: "type mismatch",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: null,
}"""

    def _assert_consumer_contract(self, verdict):
        """The pre-existing keys β/D4 consume must survive every change.

        `blocking`/`disposition` joined this contract in the task #7369
        review: `blocking` is the field that finally carries dropped-leaf
        labels to a caller, and `disposition` is the documented PASS/BLOCKS/
        INCOMPLETE headline — both must be present on every return path, not
        just the aggregation-tail one.
        """
        for key in ("blocks", "blocking", "leaf_verdicts", "summary", "disposition"):
            self.assertIn(key, verdict,
                          f"consumer-contract key {key!r} missing; got {sorted(verdict)}")
        self.assertIsInstance(verdict["blocks"], bool)
        self.assertIsInstance(verdict["blocking"], list)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertIsInstance(verdict["summary"], str)
        self.assertIsInstance(verdict["disposition"], str)

    # ── (A) a mixed batch is INCOMPLETE, not PASS ────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_reports_probed_counts(self):
        """One of two leaves was probed — the return must say exactly that."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        self._assert_consumer_contract(verdict)
        self.assertEqual(verdict.get("leaves_total"), 2)
        self.assertEqual(verdict.get("leaves_probed"), 1)
        self.assertEqual(verdict.get("leaves_unenumerated"), 1)

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_names_the_unprobed_leaf(self):
        """The never-probed leaf is nameable straight off the return value."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        self.assertEqual(verdict.get("unenumerated_leaves"),
                         ["zero-premise leaf (beta)"])

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_disposition_is_incomplete_and_does_not_block(self):
        """INCOMPLETE is the third outcome: nothing falsified, nothing verified."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        self.assertEqual(verdict.get("disposition"), "INCOMPLETE")
        self.assertFalse(verdict["blocks"],
                         "an incomplete batch has falsified nothing — it must not block")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_summary_states_the_probed_count(self):
        """The one-line summary carries the count a reader actually needs."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        summary = verdict["summary"]
        self.assertIn("1 of 2", summary, f"summary must state 1 of 2; got {summary!r}")
        self.assertIn("zero-premise leaf (beta)", summary,
                      f"summary must name the never-probed leaf; got {summary!r}")
        self.assertNotIn("PASS", summary, f"INCOMPLETE must not read PASS; got {summary!r}")

    # ── (B) an all-verified batch still passes, and says on what basis ───────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_all_verified_batch_disposition_is_pass(self):
        """The happy path is unchanged in outcome — regression guard."""
        verdict, _ = self._run_scenario(self._B_LEAVES, self.VERIFIED_RESPONSES)
        self._assert_consumer_contract(verdict)
        self.assertEqual(verdict.get("disposition"), "PASS")
        self.assertFalse(verdict["blocks"])
        self.assertEqual(verdict.get("leaves_probed"), verdict.get("leaves_total"))
        self.assertEqual(verdict.get("leaves_total"), 2)
        self.assertEqual(verdict.get("leaves_unenumerated"), 0)

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_all_verified_summary_reports_the_probed_count(self):
        """Even a pass states its basis, so 'verified' is never taken on trust."""
        verdict, _ = self._run_scenario(self._B_LEAVES, self.VERIFIED_RESPONSES)
        summary = verdict["summary"]
        self.assertIn("PASS", summary, f"an all-verified batch passes; got {summary!r}")
        self.assertIn("2 probed", summary,
                      f"summary must state the probed count; got {summary!r}")

    # ── (C) a leaf that executed nothing is NOT_VERIFIED, batch INCOMPLETE ───

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_leaf_with_no_executed_records_is_not_verified(self):
        """executed === 0 with malformed records ⇒ that leaf verified nothing."""
        verdict, _ = self._run_scenario(self._C_LEAVES, self._C_RESPONSES)
        self._assert_consumer_contract(verdict)
        leaf = verdict["leaf_verdicts"][0]
        self.assertEqual(leaf.get("disposition"), "NOT_VERIFIED",
                         f"leaf that executed no probe must be NOT_VERIFIED; got {leaf!r}")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_malformed_records_are_counted_at_batch_level(self):
        """The harness-defect count is visible without walking leaf_verdicts."""
        verdict, _ = self._run_scenario(self._C_LEAVES, self._C_RESPONSES)
        self.assertEqual(verdict.get("malformed_records"), 1)
        self.assertEqual(verdict.get("fixture_absent_records"), 0)

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_not_verified_leaf_makes_the_batch_incomplete_without_blocking(self):
        """A harness defect is neither a pass nor a falsification."""
        verdict, _ = self._run_scenario(self._C_LEAVES, self._C_RESPONSES)
        self.assertEqual(verdict.get("disposition"), "INCOMPLETE")
        self.assertFalse(verdict["blocks"],
                         "a malformed record is a harness defect, not a falsification")
        self.assertEqual(verdict.get("leaves_not_verified"), 1)
        self.assertEqual(verdict.get("not_verified_leaves"),
                         ["malformed-records leaf (epsilon)"])

    # ── (D) a blocking leaf that executed nothing is not a probed leaf ───────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_null_synthesize_leaf_blocks_but_is_not_counted_as_probed(self):
        """BLOCKS does not imply evidence — leaves_probed reads the counter.

        The null-synthesize fallback blocks the leaf without any harness output,
        so `executed` is 0 by construction.  Counting it as probed inflated the
        coverage number precisely when the pipeline was broken.
        """
        verdict, _ = self._run_scenario(self._D_LEAVES, self._D_RESPONSES)
        self._assert_consumer_contract(verdict)
        self.assertTrue(verdict["blocks"], "a dropped synthesize must fail closed")
        self.assertEqual(verdict.get("disposition"), "BLOCKS")
        self.assertEqual(verdict.get("leaves_total"), 1)
        self.assertEqual(verdict.get("leaves_probed"), 0,
                         "nothing executed for this leaf; it must not count as probed")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_null_synthesize_leaf_reports_zero_executed(self):
        """The fallback verdict states its own (empty) basis."""
        verdict, _ = self._run_scenario(self._D_LEAVES, self._D_RESPONSES)
        leaf = verdict["leaf_verdicts"][0]
        self.assertEqual(leaf.get("executed"), 0)
        self.assertEqual(leaf.get("total"), 0)
        self.assertEqual(leaf.get("blocking"), ["null-synthesize leaf (zeta)"])

    # ── (E) a leaf the pipeline itself dropped is named in `blocking` (#7369) ─

    _E_LEAVES = '[{ signal: "ok leaf (eta)" }, { signal: "doomed leaf (theta)" }]'
    _E_RESPONSES = """{
    enumerate: (prompt) => {
        if (prompt.includes("doomed")) { throw new Error("enumerator exploded"); }
        return { premises: [{
            text: "revolute rejects non-axis arg",
            assertion_kind: "rejection",
            fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
            match: { exit_code: 1 },
            capability: "arg-vs-param rejection (mock)",
        }] };
    },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check", verdict: "PASS",
        command: ["reify", "check", "f.ri"], exit_code: 1,
        stdout: "", stderr: "type mismatch",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [], report: "",
                  malformed: [], fixture_absent: [], executed: 1, total: 1 },
}"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_pipeline_dropped_leaf_is_named_in_batch_level_blocking(self):
        """The dropped leaf's label must escape droppedLeafLabels() and reach
        the return value a caller actually consumes (task #7369 review):
        computing the label and then discarding it was behaviorally a no-op,
        observable only by the pure-unit test on the helper itself.
        """
        verdict, _ = self._run_scenario(self._E_LEAVES, self._E_RESPONSES)
        self._assert_consumer_contract(verdict)
        self.assertTrue(verdict["blocks"], "a pipeline-dropped leaf must fail closed")
        self.assertEqual(verdict.get("disposition"), "BLOCKS")
        self.assertIn("blocking", verdict,
                      f"batch verdict has no blocking; keys {sorted(verdict)}")
        # "doomed leaf (theta)" is leaves[1] — the label must carry that index.
        self.assertEqual(verdict["blocking"], ["<dropped-leaf:1:doomed leaf (theta)>"])

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_pipeline_dropped_leaf_does_not_shrink_leaves_total(self):
        """`leaves_total` must count the SUBMITTED batch, not just the leaves
        that survived to `filtered` (task #7369 review): before this fix,
        leaves_total was filtered.length, so this exact 2-leaf/1-dropped
        scenario reported leaves_total: 1 — reading as full coverage — while
        `blocking` simultaneously named a leaf that was never adjudicated.
        Only "ok leaf (eta)" (leaves[0]) actually ran a probe, so
        leaves_probed must stay 1 even though leaves_total is now 2.
        """
        verdict, _ = self._run_scenario(self._E_LEAVES, self._E_RESPONSES)
        self.assertEqual(verdict.get("leaves_total"), 2,
                         "leaves_total must count the whole submitted batch, dropped or not")
        self.assertEqual(verdict.get("leaves_probed"), 1,
                         "the dropped leaf never ran a probe and must not count as probed")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_pipeline_dropped_leaf_logs_the_caught_error(self):
        """The mock pipeline()'s catch must record the throw, not swallow it
        (task #7369 review): a genuine .mjs stage defect and an intentionally
        -thrown mock error both used to look identical — a dropped leaf with
        no trace of why. Assert the throw path actually fired, the way the
        real runtime's own `pipeline[<index>] failed: <message>` record would
        let a reader confirm it, instead of only inferring it from the verdict.

        This is a harness self-check: it verifies the scenario mock's own
        catch block, not any behavior of prd-decompose-verify.mjs, so it
        reads the dedicated __PIPELINE_ERRORS channel (task #7369 review round
        2) rather than __LOG_LINES — the latter is reserved for the .mjs's own
        log() output and must never carry harness-injected text.
        """
        errors = self._run_scenario_pipeline_errors(self._E_LEAVES, self._E_RESPONSES)
        combined = "\n".join(errors)
        self.assertIn(
            "pipeline[1] failed: enumerator exploded", combined,
            f"expected the caught enumerator error at index 1; got {errors!r}",
        )

    # ── (F) zero leaves — the empty-batch early return owes the same contract ─

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_zero_leaf_batch_has_full_consumer_contract(self):
        """The empty-batch early return must not omit fields a consumer would
        reasonably read off any other return path (task #7369 review): before
        this test, `blocking`/`disposition`/the counters existed only on the
        aggregation-tail path, so `verdict.blocking.length` or
        `verdict.disposition === 'PASS'` was `undefined` on a zero-leaf batch
        instead of the vacuously-true answer.
        """
        verdict, phases = self._run_scenario("[]", "{}")
        self._assert_consumer_contract(verdict)
        self.assertEqual(verdict.get("disposition"), "PASS")
        self.assertFalse(verdict["blocks"])
        self.assertEqual(verdict.get("blocking"), [])
        self.assertEqual(verdict.get("leaves_total"), 0)
        self.assertEqual(verdict.get("leaves_probed"), 0)
        self.assertEqual(phases, [], "zero leaves must never invoke any agent stage")

        # _assert_consumer_contract only pins a hand-picked subset of keys —
        # exactly the kind of subset check that let the zero-leaf path drift
        # from the aggregation tail's key set in the first place (task #7369
        # review round 2). Pin the FULL key set instead, against a populated
        # scenario that goes through the normal tail: this single assertion
        # subsumes any per-key list and cannot itself drift, because both
        # paths now build through the shared batchVerdict() constructor.
        populated, _ = self._run_scenario(self._B_LEAVES, self.VERIFIED_RESPONSES)
        self.assertEqual(
            set(verdict), set(populated),
            "zero-leaf return must expose the same keys as the aggregation tail",
        )


if __name__ == "__main__":
    unittest.main()
