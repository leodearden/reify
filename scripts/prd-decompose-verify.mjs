// prd-decompose-verify.mjs — γ Workflow-tool script (PRD §4 D3 / §11 γ)
//
// Thin orchestration shell over the deterministic Python harness
// (scripts/prd-decompose-verify.py) and the α probe runner
// (scripts/prd-capability-check.py).
//
// Design (PRD D2):
//   "a deterministic harness over stochastic agents: agents find+author probes,
//   D1 adjudicates."  All load-bearing logic (negative-assertion binding,
//   blocking synthesis, captured-output report) lives in the tested Python
//   harness; this .mjs is a thin orchestration shell.
//
// Per-leaf pipeline:
//   Enumerator  — extract premises from leaf signal; enforce negative-assertion
//                 mandate; emit {premises:[...]} JSON
//   Prover ‖ Adversary  — run concurrently:
//     Prover:   receive premises inline; write to temp file; invoke
//               `prd-decompose-verify.py bind` then `prd-capability-check.py
//               --json`; return result records via RESULTS_SCHEMA
//     Adversary: independent lens — hunt unlisted premises + falsifications;
//               return its own α result records via RESULTS_SCHEMA
//   Synthesize — agent receives combined records inline; writes to temp file;
//               runs `prd-decompose-verify.py synthesize` (deterministic);
//               returns BatchVerdict via VERDICT_SCHEMA
//
// Uses ONLY Workflow-injected globals: agent, parallel, pipeline, log, phase,
// args, budget, workflow.  Does NOT use tmp_file or shell (not injected).
//
// Batch verdict: blocks on any FAIL/UNPROVABLE/HARNESS_ERROR from any leaf that
// carries executed-probe evidence.  The script returns a summary object with
// per-leaf verdicts and aggregate blocking status.
//
// Two lines of defence against a verdict with no evidence behind it:
//   1. RESULT_RECORD_SCHEMA (below) — an agent cannot EMIT a record without a
//      capability, a verdict from the closed vocabulary, an argv-array command
//      and an integer exit_code.
//   2. The Python harness (has_probe_evidence / classify_record) — anything
//      that still arrives evidence-free is reported as MALFORMED rather than
//      tabulated as a premise falsification (PRD §6 decision 4).
//
// Committed under scripts/ (not .claude/workflows/ which is .gitignored) so
// β can reference it by a stable path and D4 can re-run it.  .mjs extension
// lets `node --check` parse it as ESM regardless of package.json.

export const meta = {
    name: "prd-decompose-verify",
    description: "γ: per-leaf premise verification — Enumerator → Prover‖Adversary → Synthesize",
    phases: [
        { title: "Enumerate", detail: "Extract premises from each leaf signal", model: "sonnet" },
        { title: "Prove",     detail: "Author probes and run via prd-capability-check.py", model: "sonnet" },
        { title: "Adversary", detail: "Independent lens: hunt unlisted premises/falsifications", model: "opus" },
        { title: "Synthesize", detail: "Deterministic synthesis: block on FAIL/UNPROVABLE/HARNESS_ERROR", model: "haiku" },
    ],
};

// ---------------------------------------------------------------------------
// JSON schemas for structured agent output
// ---------------------------------------------------------------------------

const PREMISES_SCHEMA = {
    type: "object",
    required: ["premises"],
    properties: {
        premises: {
            type: "array",
            items: {
                type: "object",
                required: ["text", "assertion_kind", "fixture", "match"],
                properties: {
                    text:           { type: "string" },
                    assertion_kind: { type: "string",
                                      enum: ["rejection", "parses", "resolves", "produces", "ir"] },
                    fixture:        { type: "string" },
                    match:          { type: "object" },
                    capability:     { type: "string" },
                },
            },
        },
    },
};

// One α --json result record (prd-capability-check.py's --json record shape).
//
// This is the FIRST line of defence for the evidence gate; the Python harness
// (has_probe_evidence / classify_record) is the second.  Left as the former
// `{type:"object"}`, the schema accepted three shapes that all reached the
// synthesis step and were tabulated as premise falsifications:
//
//   - a PREMISE record (no verdict key at all) validating as a RESULT record;
//   - a record with no `command`/`exit_code` — an unexecuted promise;
//   - `command: "target/release/reify eval f.ri"` (a STRING), which the report
//     builder then rendered character-by-character.
//
// Requiring capability/verdict/command/exit_code, pinning `verdict` to the
// PRD §6 decision 3 vocabulary, and typing `command` as an array of strings
// makes all three unrepresentable at the agent boundary.
const RESULT_RECORD_SCHEMA = {
    type: "object",
    required: ["capability", "verdict", "command", "exit_code"],
    properties: {
        capability: { type: "string" },
        probe_kind: { type: "string" },
        verdict:    { type: "string",
                      enum: ["PASS", "FAIL", "UNPROVABLE", "HARNESS_ERROR"] },
        // argv TOKENS, never a ready-to-paste shell string.
        command:    { type: "array", items: { type: "string" } },
        // A real process outcome.  -1 when no process ran (HARNESS_ERROR).
        exit_code:  { type: "integer" },
        stdout:     { type: "string" },
        stderr:     { type: "string" },
    },
};

const RESULTS_SCHEMA = {
    type: "object",
    required: ["prover"],
    properties: {
        prover:    { type: "array", items: RESULT_RECORD_SCHEMA },
        adversary: { type: "array", items: RESULT_RECORD_SCHEMA },
    },
};

// `malformed`, `fixture_absent`, `executed` and `total` are DECLARED but NOT
// required: the Synthesize agent shells the Python harness and returns its JSON
// verbatim, so requiring them would hard-fail against any harness build that
// predates them.  Stage 3 defaults them with `?? []` / `?? 0` instead.
const VERDICT_SCHEMA = {
    type: "object",
    required: ["blocks", "blocking", "report"],
    properties: {
        blocks:         { type: "boolean" },
        blocking:       { type: "array", items: { type: "string" } },
        report:         { type: "string" },
        malformed:      { type: "array", items: { type: "string" } },
        fixture_absent: { type: "array", items: { type: "string" } },
        executed:       { type: "integer" },
        total:          { type: "integer" },
    },
};

// ---------------------------------------------------------------------------
// normalizeLeaves — defensive args→leaves normalization (task #4960)
//
// The Workflow tool is documented to invoke this script with args set to the
// leaf array directly, but args has been observed arriving JSON-STRINGIFIED
// (3x). Array.isArray(<string>) is false, so a naive `args ? [args] : []`
// fallback wraps the ENTIRE stringified batch as ONE mega-leaf, silently
// destroying per-leaf fan-out (mega-leaf trap): one Enumerator confidence-
// filters premises from a giant blob (recall loss) and one Prover/Adversary
// pass runs instead of N.
//
// This helper is a PURE function of its two params (rawArgs, warn) — no
// injected Workflow globals, no Node APIs — so it can be unit-tested in
// isolation. Every non-string input (real array, single object leaf,
// undefined/null) falls through to the exact pre-fix expression unchanged.
// ---------------------------------------------------------------------------

function normalizeLeaves(rawArgs, warn) {
    let value = rawArgs;
    if (typeof rawArgs === "string") {
        try {
            const parsed = JSON.parse(rawArgs);
            if (Array.isArray(parsed)) {
                value = parsed; // stringified leaf array -> real array: fan-out restored
            } else {
                warn("normalizeLeaves: args arrived as a JSON string that parsed to a "
                    + "non-array value; treating the whole string as ONE mega-leaf — "
                    + "per-leaf fan-out is DEGRADED.");
                // keep value = rawArgs (original single-leaf fallback)
            }
        } catch (e) {
            warn("normalizeLeaves: args arrived as a non-JSON string; treating the "
                + "whole string as ONE mega-leaf — per-leaf fan-out is DEGRADED. "
                + "JSON.parse error: " + e.message);
            // keep value = rawArgs (original single-leaf fallback)
        }
    }
    return Array.isArray(value) ? value : (value ? [value] : []);
}

// ---------------------------------------------------------------------------
// Main workflow body
//
// The Workflow harness wraps this script body in an async function and takes
// the aggregate verdict from its top-level `return` (every Workflow doc example
// ends `return {...}`). A top-level `return` IS required here — it is how the
// harness receives the result.
//
// IMPORTANT: raw `node --check <file>` and `await import(<file>)` both reject
// the top-level `return` with SyntaxError: Illegal return statement (valid ESM
// has no top-level `return`). Syntax and runtime validation must use the wrapped
// form: strip `export const meta` → `const meta`, wrap the body in an
// `async function __wf() { ... }`, then node --check / AsyncFunction that.
// ---------------------------------------------------------------------------

const _wfResult = await (async function runWorkflow() {

    // `args` is injected by the Workflow harness. Normalize defensively: args
    // has been observed arriving JSON-stringified (task #4960 mega-leaf trap).
    const leaves = normalizeLeaves(args, log); // eslint-disable-line no-undef

    if (leaves.length === 0) {
        log("No leaves provided — γ verification skipped."); // eslint-disable-line no-undef
        return { blocks: false, leaf_verdicts: [], summary: "No leaves to verify." };
    }

    log(`γ verification: ${leaves.length} leaf(ves)`); // eslint-disable-line no-undef

    // Every role pins its own model. Left unpinned, an agent() call inherits the
    // main-loop model and session effort, so a /prd decompose driven from a Fable
    // window ran all four roles on Fable — 8 leaves × 4 roles = 41.7M tokens on
    // 2026-08-26, of which the Adversary alone was 24.4M. Enumerate/Prove are
    // bounded (read one leaf; shell the α runner), Synthesize just shells the
    // deterministic harness. The Adversary is the one role held at the top tier:
    // it is the adversarial-verify stage, and a downgraded adversary is how a
    // verification gate goes quietly vacuous while still reporting PASS.

    // ── per-leaf pipeline: Enumerate → (Prove ‖ Adversary) → Synthesize ──────

    const leaf_verdicts = await pipeline( // eslint-disable-line no-undef
        leaves,

        // Stage 1: Enumerator — extract premises from leaf signal
        async (leaf, originalLeaf, idx) => {
            const leafLabel = typeof leaf === "string" ? leaf
                : (leaf.signal || leaf.text || `leaf-${idx}`);

            const enumerated = await agent( // eslint-disable-line no-undef
                `You are the Enumerator for γ decompose-phase verification (PRD §11 γ).

Your task: given the following decompose leaf signal, extract every premise it
asserts as a structured list.

LEAF SIGNAL:
${JSON.stringify(leaf, null, 2)}

Instructions:
1. Read the leaf signal carefully.
2. For each factual claim or behavioral assertion, create one premise record.
3. Enforce the NEGATIVE-ASSERTION MANDATE: for any "X is rejected" or "X should
   fail" assertion, use assertion_kind="rejection" with match.exit_code=1.
   DO NOT use observation="absent" for rejection premises — that would silently
   pass the 4575 silent-accept bug.
4. assertion_kind values:
   - "rejection": compiler must reject (exit_code:1) — e.g. type mismatches
   - "parses":    tree-sitter parses the fixture without errors
   - "resolves":  reify check passes (exit_code:0)
   - "produces":  reify eval exits non-zero with this signature in stderr
   - "ir":        reify eval exits 0 (clean, no error) — observation=absent
5. Each premise needs a fixture path (repo-relative). If the leaf doesn't
   specify one, you may need to reference an existing fixture in
   tests/prd-gate/fixtures/ or note that a new fixture is needed.
6. Return ONLY premises you are confident about. An empty list is valid.

Return a JSON object {premises: [...]} matching the schema.`,
                { label: `enumerate:${idx}`, phase: "Enumerate", schema: PREMISES_SCHEMA, model: "sonnet" }
            );

            return { leaf, leafLabel, enumerated, idx };
        },

        // Stage 2: Prover ‖ Adversary (concurrent)
        // Premises are passed INLINE as JSON. Each agent uses its own tools
        // to write temp files and shell out — no tmp_file/shell globals needed.
        async ({ leaf, leafLabel, enumerated, idx }) => {
            if (!enumerated || !enumerated.premises || enumerated.premises.length === 0) {
                // Carry the reason forward. Without this marker stage 3 cannot tell
                // "nothing was asserted" from "everything asserted held", because
                // synthesizing an empty record set is vacuously non-blocking.
                log(`[${idx}] UNENUMERATED — Enumerator returned zero premises for leaf: ${leafLabel}. ` // eslint-disable-line no-undef
                    + `NO probe will run and this leaf is NOT verified.`);
                return { leaf, leafLabel, idx, prover: [], adversary: [], unenumerated: true };
            }

            const premisesJson = JSON.stringify(enumerated, null, 2);

            const [proverOut, adversaryOut] = await parallel([ // eslint-disable-line no-undef
                // Prover: write premises to temp file, bind, run α, return records
                async () => agent( // eslint-disable-line no-undef
                    `You are the Prover for γ decompose-phase verification.

Your task: bind the enumerated premises to a probe-set and run it through the α
probe runner, then return the result records.

LEAF: ${leafLabel}
PREMISES JSON:
${premisesJson}

Steps (use your own shell/file tools):
1. Write the PREMISES JSON above to a temp file, e.g.:
     echo '<premises_json>' > /tmp/pdv_premises_${idx}.json
2. Run: python3 scripts/prd-decompose-verify.py bind /tmp/pdv_premises_${idx}.json
   Capture stdout (the probe-set JSON). If exit code != 0, return a HARNESS_ERROR record.
3. Write the probe-set JSON to another temp file, e.g.:
     /tmp/pdv_probeset_${idx}.json
4. Run: python3 scripts/prd-capability-check.py --json /tmp/pdv_probeset_${idx}.json
   Capture the full stdout JSON. Parse the "results" array from it.
5. Return {prover: [result_records...], adversary: []}.

RECORD SHAPE — every result record MUST carry all four of capability, verdict,
command and exit_code, and:
  - \`command\` MUST be an ARRAY OF STRINGS (argv tokens, e.g.
    ["python3", "scripts/prd-capability-check.py", "--json", "/tmp/ps.json"]).
    Do NOT return a single ready-to-paste shell string — it is not re-runnable
    as captured evidence and the schema rejects it.
  - \`exit_code\` MUST be an INTEGER (use -1 when no process ran).  Never null.
  - \`verdict\` MUST be one of PASS / FAIL / UNPROVABLE / HARNESS_ERROR.
Pass through α's captured command/exit_code/stdout/stderr VERBATIM — do not
reconstruct, re-quote or summarize them.  A blocking verdict with no captured
command+exit_code is discarded by the harness as an unexecuted promise, so an
invented record wins you nothing.

If any step fails, return a single HARNESS_ERROR result record:
  {capability: "${leafLabel}", probe_kind: "check", verdict: "HARNESS_ERROR",
   command: [], exit_code: -1, stdout: "", stderr: "<error detail>"}`,
                    { label: `prove:${idx}`, phase: "Prove", schema: RESULTS_SCHEMA, model: "sonnet", effort: "medium" }
                ),

                // Adversary: independent lens
                async () => agent( // eslint-disable-line no-undef
                    `You are the Adversary for γ decompose-phase verification.

Your task: independently examine the leaf signal and hunt for premises that the
Prover may have missed, or attempt to FALSIFY the enumerated premises.

LEAF SIGNAL:
${JSON.stringify(leaf, null, 2)}

ENUMERATED PREMISES (what the Prover checked):
${premisesJson}

Instructions:
1. Are there any premises NOT listed above that should hold? If so, bind them
   to probes and run them via prd-capability-check.py --json using your own tools.
2. Are any of the enumerated premises stated with the WRONG polarity (e.g., a
   rejection premise bound to observation="absent" instead of "present")?
   Flag these as FAIL records.
3. Return any additional result records as the "adversary" field.
4. You can only ADD blocking signals — if you find nothing new, return empty
   adversary list.

RECORD SHAPE — every result record MUST carry all four of capability, verdict,
command and exit_code, and:
  - \`command\` MUST be an ARRAY OF STRINGS (argv tokens), never a single
    ready-to-paste shell string.
  - \`exit_code\` MUST be an INTEGER (use -1 when no process ran).  Never null.
  - \`verdict\` MUST be one of PASS / FAIL / UNPROVABLE / HARNESS_ERROR.
A FAIL you did not actually RUN is not a falsification — the harness discards
any blocking record with no captured command+exit_code as an unexecuted
promise.  Report only what you probed, with α's captured output verbatim.

Return JSON: {prover: [], adversary: [result_records...]}`,
                    { label: `adversary:${idx}`, phase: "Adversary", schema: RESULTS_SCHEMA, model: "opus", effort: "xhigh" }
                ),
            ]);

            const proverRecords = (proverOut && proverOut.prover) ? proverOut.prover : [];
            const adversaryRecords = (adversaryOut && adversaryOut.adversary) ? adversaryOut.adversary : [];

            return { leaf, leafLabel, idx, prover: proverRecords, adversary: adversaryRecords };
        },

        // Stage 3: Synthesize — agent receives combined records inline, runs
        // deterministic harness, returns BatchVerdict via VERDICT_SCHEMA.
        async ({ leaf, leafLabel, idx, prover, adversary, unenumerated }) => {
            // A leaf whose Enumerator produced nothing was never probed. Do NOT
            // pay a Synthesize agent to adjudicate an empty record set: α over {}
            // is vacuously non-blocking, so the call could only ever come back
            // clean — which is precisely how "nothing ran" gets laundered into
            // "nothing failed". Short-circuit with an explicit disposition.
            if (unenumerated) {
                log(`[${idx}] ${leafLabel}: UNENUMERATED — no probe executed.`); // eslint-disable-line no-undef
                return {
                    leafLabel,
                    blocks: false,
                    blocking: [],
                    report: `${leafLabel} — Enumerator returned zero premises; NO probe `
                        + `was executed for this leaf. This is NOT a verified pass.`,
                    disposition: "UNENUMERATED",
                    malformed: [],
                    fixture_absent: [],
                    executed: 0,
                    total: 0,
                };
            }

            const resultsJson = JSON.stringify({ prover, adversary }, null, 2);

            const synthesized = await agent( // eslint-disable-line no-undef
                `You are the Synthesize step for γ decompose-phase verification.

Your task: run the deterministic synthesis harness and return the BatchVerdict.

LEAF: ${leafLabel}
COMBINED RESULTS JSON:
${resultsJson}

Steps (use your own shell/file tools):
1. Write the COMBINED RESULTS JSON above to a temp file, e.g.:
     /tmp/pdv_results_${idx}.json
2. Run: python3 scripts/prd-decompose-verify.py synthesize /tmp/pdv_results_${idx}.json
   Capture stdout VERBATIM.
3. Parse the stdout as JSON — it is a BatchVerdict object
     {blocks, blocking, report, malformed, fixture_absent, executed, total}.
4. Return that object VERBATIM, including malformed, fixture_absent, executed
   and total. Do NOT summarize or alter the report field, do NOT drop fields you
   do not recognize, and do NOT recompute \`blocks\` yourself — the harness is
   the adjudicator and you are relaying its answer.

NOTE: exit code 0 from the harness does NOT mean "verified". Malformed and
fixture-absent records do not block, so a batch can exit 0 having probed
nothing. Relay executed/total unchanged so the caller can tell the difference.

If the command fails or stdout is not valid JSON, return:
  {blocks: true, blocking: ["${leafLabel}"], report: "<error from synthesize>"}`,
                { label: `synthesize:${idx}`, phase: "Synthesize", schema: VERDICT_SCHEMA, model: "haiku", effort: "medium" }
            );

            const verdict = synthesized || {
                blocks: true,
                blocking: [leafLabel],
                report: `synthesize agent returned null for leaf: ${leafLabel}`,
            };

            // Per-leaf disposition. `blocks: false` is NOT the same as verified:
            // the harness does not block on MALFORMED or fixture-absent records,
            // so a leaf can come back clean having executed no probe at all.
            //   BLOCKS       — an evidence-backed falsification
            //   NOT_VERIFIED — nothing was executed (malformed / fixture-absent only)
            //   VERIFIED     — at least one probe ran and nothing blocked
            // The new fields default (`?? []` / `?? 0`) so a Synthesize agent
            // relaying an older harness build still produces a usable verdict.
            const executed = verdict.executed ?? 0;
            const disposition = verdict.blocks
                ? "BLOCKS"
                : (executed === 0 ? "NOT_VERIFIED" : "VERIFIED");

            log(`[${idx}] ${leafLabel}: ${disposition}` // eslint-disable-line no-undef
                + (verdict.blocking && verdict.blocking.length > 0 ? ` — ${verdict.blocking.join(", ")}` : ""));

            return {
                leafLabel,
                ...verdict,
                disposition,
                malformed: verdict.malformed ?? [],
                fixture_absent: verdict.fixture_absent ?? [],
                executed,
                total: verdict.total ?? 0,
            };
        },
    );

    // ── Aggregate batch verdict ──────────────────────────────────────────────

    const filtered = leaf_verdicts.filter(Boolean);

    // Fail closed on dropped leaves: a leaf whose Enumerate/Prove stage raised
    // (agent death, malformed output) is dropped to null by the pipeline and
    // filtered out above.  Treating 'could not evaluate' as PASS is a false
    // negative for a verification gate — block instead.
    const dropped = leaves.length - filtered.length;
    const droppedBlocking = Array.from({ length: dropped }, (_, i) => {
        const originalIdx = leaf_verdicts.findIndex((v, j) => !v && j >= (leaves.length - dropped - i));
        return `<dropped-leaf:${originalIdx >= 0 ? originalIdx : "?"}>`;
    });

    const anyBlocks = dropped > 0 || filtered.some(v => v.blocks);
    const allBlocking = [
        ...droppedBlocking,
        ...filtered.filter(v => v.blocks).flatMap(v => v.blocking || []),
    ];

    // ── Probed-vs-unprobed accounting (task #7257 ARM 2) ─────────────────────
    //
    // `blocks: false` was the ONLY batch-level signal, and it cannot distinguish
    // "every premise held" from "no premise was ever checked" — a batch of
    // zero-premise leaves reported "γ PASS — all N leaf(ves) verified".  These
    // counters make the basis of the verdict readable straight off the return,
    // without opening journal.jsonl.
    const leaves_total = filtered.length;
    const unenumerated_leaves = filtered
        .filter(v => v.disposition === "UNENUMERATED")
        .map(v => v.leafLabel);
    const not_verified_leaves = filtered
        .filter(v => v.disposition === "NOT_VERIFIED")
        .map(v => v.leafLabel);
    // A leaf counts as probed only when a probe actually executed: BLOCKS and
    // VERIFIED both required executed evidence, UNENUMERATED and NOT_VERIFIED
    // did not.
    const leaves_probed = filtered.filter(
        v => v.disposition === "VERIFIED" || v.disposition === "BLOCKS").length;
    const leaves_unenumerated = unenumerated_leaves.length;
    const leaves_not_verified = not_verified_leaves.length;

    const malformed_records = filtered.reduce(
        (n, v) => n + (v.malformed ?? []).length, 0);
    const fixture_absent_records = filtered.reduce(
        (n, v) => n + (v.fixture_absent ?? []).length, 0);

    // Three outcomes, not two.  INCOMPLETE sits between BLOCKS and PASS: nothing
    // was falsified, but nothing was verified either, so it is NOT a pass.
    const disposition = anyBlocks
        ? "BLOCKS"
        : ((leaves_unenumerated > 0 || leaves_not_verified > 0
            || malformed_records > 0 || fixture_absent_records > 0)
            ? "INCOMPLETE"
            : "PASS");

    const unprobedLabels = [...unenumerated_leaves, ...not_verified_leaves];

    let summary;
    if (disposition === "BLOCKS") {
        summary = `γ BLOCKS — ${allBlocking.length} premise(s)/leaf(ves) failed or dropped`
            + (dropped > 0 ? ` (${dropped} leaf(ves) dropped by pipeline errors)` : "");
    } else if (disposition === "INCOMPLETE") {
        summary = `γ INCOMPLETE — ${leaves_probed} of ${leaves_total} leaf(ves) had a `
            + `probe executed; nothing was falsified, but the unprobed remainder is `
            + `NOT verified and this is NOT a pass`
            + (unprobedLabels.length > 0
                ? `. Never probed: ${unprobedLabels.join(", ")}` : "")
            + (malformed_records > 0
                ? `. ${malformed_records} record(s) had no executed-probe evidence` : "")
            + (fixture_absent_records > 0
                ? `. ${fixture_absent_records} probe(s) could not find their fixture` : "");
    } else {
        summary = `γ PASS — all ${leaves_total} leaf(ves) verified (${leaves_probed} probed)`;
    }

    log(summary); // eslint-disable-line no-undef

    return {
        blocks: anyBlocks,
        leaf_verdicts: filtered,
        summary,
        disposition,
        leaves_total,
        leaves_probed,
        leaves_unenumerated,
        leaves_not_verified,
        unenumerated_leaves,
        not_verified_leaves,
        malformed_records,
        fixture_absent_records,
    };

})();
return _wfResult;
