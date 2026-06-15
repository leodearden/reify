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
//   harness; this .mjs is a thin orchestration shell whose only CI-tested
//   contract is node --check syntax validity.
//
// Per-leaf pipeline:
//   Enumerator  — extract premises from leaf signal; enforce negative-assertion
//                 mandate; emit {premises:[...]} JSON
//   Prover ‖ Adversary  — run concurrently:
//     Prover:   author probe fixtures, invoke `prd-decompose-verify.py bind`
//               then `prd-capability-check.py --json`, return result records
//     Adversary: independent lens — hunt unlisted premises + falsifications;
//               return its own α result records
//   Synthesize — call `prd-decompose-verify.py synthesize` (deterministic) on
//               the combined Prover + Adversary results; extract BatchVerdict
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

// ---------------------------------------------------------------------------
// Helpers (use Workflow-injected globals: agent, parallel, pipeline, log,
// tmp_file, shell — undefined under node --check but valid ESM syntax)
// ---------------------------------------------------------------------------

/**
 * Write data to a temp file and return the path.
 * Uses the `tmp_file` workflow global injected by the harness.
 */
async function writeTempJson(data) {
    const json = JSON.stringify(data);
    return await tmp_file({ content: json, extension: ".json" }); // eslint-disable-line no-undef
}

/**
 * Run `python3 scripts/prd-decompose-verify.py` with the given subcommand and
 * arguments, returning captured output.
 * Uses the `shell` workflow global injected by the harness.
 */
async function runHarness(...cmdArgs) {
    return await shell({ // eslint-disable-line no-undef
        command: ["python3", "scripts/prd-decompose-verify.py", ...cmdArgs],
        capture_output: true,
    });
}

// ---------------------------------------------------------------------------
// Main workflow body — wrapped in async IIFE so `return` is syntactically valid
// (top-level `return` is illegal in ESM; `node --check` enforces this).
// The Workflow harness resolves the returned Promise as the script's result.
// ---------------------------------------------------------------------------

const _workflowResult = await (async function runWorkflow() {

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
        async ({ leaf, leafLabel, enumerated, idx }) => {
            if (!enumerated || !enumerated.premises || enumerated.premises.length === 0) {
                log(`[${idx}] No premises enumerated for leaf: ${leafLabel} — skipping proof.`); // eslint-disable-line no-undef
                return { leaf, leafLabel, idx, prover: [], adversary: [] };
            }

            const premisesFile = await writeTempJson(enumerated);

            const bindResult = await runHarness("bind", premisesFile);
            if (bindResult.exit_code !== 0) {
                log(`[${idx}] bind failed for leaf: ${leafLabel}`); // eslint-disable-line no-undef
                return {
                    leaf, leafLabel, idx,
                    prover: [{ capability: leafLabel, probe_kind: "check",
                                verdict: "HARNESS_ERROR", command: ["bind"],
                                exit_code: bindResult.exit_code,
                                stdout: bindResult.stdout || "",
                                stderr: bindResult.stderr || "bind failed" }],
                    adversary: [],
                };
            }

            const probeSetFile = await writeTempJson(JSON.parse(bindResult.stdout));

            const [proverOut, adversaryOut] = await parallel([ // eslint-disable-line no-undef
                // Prover: run probe-set through α
                async () => agent( // eslint-disable-line no-undef
                    `You are the Prover for γ decompose-phase verification.

Your task: run the committed probe-set through the α probe runner and return
the result records.

LEAF: ${leafLabel}
PROBE-SET FILE: ${probeSetFile}

Run: python3 scripts/prd-capability-check.py --json ${probeSetFile}

Capture the stdout JSON, parse it, and return the "results" array as the
"prover" field.

If any probe requires a .ri fixture that doesn't exist, note it in the result
as a HARNESS_ERROR record rather than failing silently.

Return JSON: {prover: [result_records...], adversary: []}`,
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
${JSON.stringify(enumerated.premises, null, 2)}

Instructions:
1. Are there any premises NOT listed above that should hold? If so, bind them
   to probes and run them via prd-capability-check.py --json.
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

        // Stage 3: Synthesize (deterministic)
        async ({ leaf, leafLabel, idx, prover, adversary }) => {
            const resultsFile = await writeTempJson({ prover, adversary });

            const synResult = await runHarness("synthesize", resultsFile);

            let verdict;
            if (synResult.exit_code === 0 || synResult.exit_code === 1) {
                try {
                    verdict = JSON.parse(synResult.stdout);
                } catch (e) {
                    verdict = {
                        blocks: true,
                        blocking: [leafLabel],
                        report: `synthesize parse error: ${e.message}\n${synResult.stdout}`,
                    };
                }
            } else {
                verdict = {
                    blocks: true,
                    blocking: [leafLabel],
                    report: `synthesize error (exit ${synResult.exit_code}): ${synResult.stderr}`,
                };
            }

            log(`[${idx}] ${leafLabel}: ${verdict.blocks ? "BLOCKS" : "PASS"}` // eslint-disable-line no-undef
                + (verdict.blocking.length > 0 ? ` — ${verdict.blocking.join(", ")}` : ""));

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
