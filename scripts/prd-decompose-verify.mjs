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
// Batch verdict: blocks on any FAIL/UNPROVABLE/HARNESS_ERROR from any leaf.
// The script returns a summary object with per-leaf verdicts and aggregate
// blocking status.
//
// Committed under scripts/ (not .claude/workflows/ which is .gitignored) so
// β can reference it by a stable path and D4 can re-run it.  .mjs extension
// lets `node --check` parse it as ESM regardless of package.json.

export const meta = {
    name: "prd-decompose-verify",
    description: "γ: per-leaf premise verification — Enumerator → Prover‖Adversary → Synthesize",
    phases: [
        { title: "Enumerate", detail: "Extract premises from each leaf signal" },
        { title: "Prove",     detail: "Author probes and run via prd-capability-check.py" },
        { title: "Adversary", detail: "Independent lens: hunt unlisted premises/falsifications" },
        { title: "Synthesize", detail: "Deterministic synthesis: block on FAIL/UNPROVABLE/HARNESS_ERROR" },
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

const RESULTS_SCHEMA = {
    type: "object",
    required: ["prover"],
    properties: {
        prover:    { type: "array", items: { type: "object" } },
        adversary: { type: "array", items: { type: "object" } },
    },
};

const VERDICT_SCHEMA = {
    type: "object",
    required: ["blocks", "blocking", "report"],
    properties: {
        blocks:   { type: "boolean" },
        blocking: { type: "array", items: { type: "string" } },
        report:   { type: "string" },
    },
};

// ---------------------------------------------------------------------------
// Main workflow body — wrapped in async IIFE so `return` is syntactically valid
// (top-level `return` is illegal in ESM; `node --check` enforces this).
//
// The IIFE is the LAST statement of the module and is left as a BARE EXPRESSION
// STATEMENT (not bound to a variable). The Workflow harness captures the script's
// result as the module body's completion value, so the resolved aggregate verdict
// {blocks, leaf_verdicts, summary} returned by runWorkflow() becomes the script's
// result. Binding it to a `const` (as a prior revision did) would discard it —
// a `const` declaration has an empty completion value — silently dropping the
// whole point of γ. Top-level `return` is not an option here because `node --check`
// rejects it as illegal in ESM, so completion-value surfacing is the only path.
// ---------------------------------------------------------------------------

await (async function runWorkflow() {

    // `args` is injected by the Workflow harness.
    const leaves = Array.isArray(args) ? args : (args ? [args] : []); // eslint-disable-line no-undef

    if (leaves.length === 0) {
        log("No leaves provided — γ verification skipped."); // eslint-disable-line no-undef
        return { blocks: false, leaf_verdicts: [], summary: "No leaves to verify." };
    }

    log(`γ verification: ${leaves.length} leaf(ves)`); // eslint-disable-line no-undef

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
                { label: `enumerate:${idx}`, phase: "Enumerate", schema: PREMISES_SCHEMA }
            );

            return { leaf, leafLabel, enumerated, idx };
        },

        // Stage 2: Prover ‖ Adversary (concurrent)
        // Premises are passed INLINE as JSON. Each agent uses its own tools
        // to write temp files and shell out — no tmp_file/shell globals needed.
        async ({ leaf, leafLabel, enumerated, idx }) => {
            if (!enumerated || !enumerated.premises || enumerated.premises.length === 0) {
                log(`[${idx}] No premises enumerated for leaf: ${leafLabel} — skipping proof.`); // eslint-disable-line no-undef
                return { leaf, leafLabel, idx, prover: [], adversary: [] };
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

If any step fails, return a single HARNESS_ERROR result record:
  {capability: "${leafLabel}", probe_kind: "check", verdict: "HARNESS_ERROR",
   command: [], exit_code: -1, stdout: "", stderr: "<error detail>"}`,
                    { label: `prove:${idx}`, phase: "Prove", schema: RESULTS_SCHEMA }
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

Return JSON: {prover: [], adversary: [result_records...]}`,
                    { label: `adversary:${idx}`, phase: "Adversary", schema: RESULTS_SCHEMA }
                ),
            ]);

            const proverRecords = (proverOut && proverOut.prover) ? proverOut.prover : [];
            const adversaryRecords = (adversaryOut && adversaryOut.adversary) ? adversaryOut.adversary : [];

            return { leaf, leafLabel, idx, prover: proverRecords, adversary: adversaryRecords };
        },

        // Stage 3: Synthesize — agent receives combined records inline, runs
        // deterministic harness, returns BatchVerdict via VERDICT_SCHEMA.
        async ({ leaf, leafLabel, idx, prover, adversary }) => {
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
3. Parse the stdout as JSON — it is a BatchVerdict object {blocks, blocking, report}.
4. Return that object directly. Do NOT summarize or alter the report field.

If the command fails or stdout is not valid JSON, return:
  {blocks: true, blocking: ["${leafLabel}"], report: "<error from synthesize>"}`,
                { label: `synthesize:${idx}`, phase: "Synthesize", schema: VERDICT_SCHEMA }
            );

            const verdict = synthesized || {
                blocks: true,
                blocking: [leafLabel],
                report: `synthesize agent returned null for leaf: ${leafLabel}`,
            };

            log(`[${idx}] ${leafLabel}: ${verdict.blocks ? "BLOCKS" : "PASS"}` // eslint-disable-line no-undef
                + (verdict.blocking && verdict.blocking.length > 0 ? ` — ${verdict.blocking.join(", ")}` : ""));

            return { leafLabel, ...verdict };
        },
    );

    // ── Aggregate batch verdict ──────────────────────────────────────────────

    const filtered = leaf_verdicts.filter(Boolean);
    const anyBlocks = filtered.some(v => v.blocks);
    const allBlocking = filtered.filter(v => v.blocks).flatMap(v => v.blocking || []);

    const summary = anyBlocks
        ? `γ BLOCKS — ${allBlocking.length} premise(s) failed across ${filtered.filter(v => v.blocks).length} leaf(ves)`
        : `γ PASS — all ${filtered.length} leaf(ves) verified`;

    log(summary); // eslint-disable-line no-undef

    return {
        blocks: anyBlocks,
        leaf_verdicts: filtered,
        summary,
    };

})();
