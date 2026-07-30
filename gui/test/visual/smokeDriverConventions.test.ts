/**
 * CI cover for `./smokeDriverConventions.ts` — the structural conventions the
 * live `smoke_*.mjs` drivers must follow (task 5857).
 *
 * Read that module's header for WHY a source-level check is the only executable
 * signal available here: a driver needs a live reify-gui (WebKit WebView +
 * OCCT), so CI can never run one and `node --check` catches syntax alone.
 *
 * The negative controls below are the load-bearing half. A source-level regex
 * check is INERT BY DEFAULT — a regex that matches too much reports the two
 * ALREADY-COMPLIANT drivers as violators, and a regex that matches too little
 * reports nobody at all; either way the constraint stops meaning what it says.
 * `open_file` appears in prose comments in all six drivers AND inside a string
 * literal that SURVIVES migration, so each of those false-positive shapes gets
 * its own case, one per mechanism.
 *
 * Violation assertions are on `code`, never on `message`: the prose is meant to
 * stay free to reword or enrich without churning the pins.
 */
import { describe, it, expect } from "vitest";

import { findSmokeDriverConventionViolations } from "./smokeDriverConventions.js";

/** The violation identities a source trips, in the order the predicate reports them. */
function codesFor(source: string): string[] {
  return findSmokeDriverConventionViolations(source).map((violation) => violation.code);
}

describe("findSmokeDriverConventionViolations — the pure predicate behind the convention", () => {
  // One case per spelling of the call the four unmigrated drivers carry. The
  // quote style differs between drivers, and the whitespace form is what a
  // formatter could introduce at any time, so anchoring on a single literal
  // spelling would let a real inline copy walk straight past the guard.
  it.each([
    [
      "a single-quoted call, as the four inline copies spell it",
      "    openResult = await rpc('open_file', { path: FIXTURE });",
    ],
    ["a double-quoted call", '    await rpc("open_file", { path: MAIN });'],
    ["a call with whitespace inside the parens", "    await rpc( 'open_file' , {} );"],
  ])("flags %s", (_form, source) => {
    expect(codesFor(source)).toEqual(["inline-open-file"]);
  });

  // THE FALSE POSITIVES. Every one of these appears verbatim in a driver that is
  // ALREADY compliant, so a predicate that flags any of them fails the corpus
  // guard for the wrong reason and would be "fixed" by editing a correct driver.
  it.each([
    [
      "a line comment naming the tool (smoke_appearance_e2e.mjs:116)",
      "  // Retry open_file up to 8 times (≤45s) to give the WebView time to complete",
    ],
    [
      "a JSDoc line naming the tool (smoke_find_uses.mjs:12)",
      " *   2. open_file — load find_uses_smoke.ri (structure Smoke { param x; let y = x + x })",
    ],
    [
      "a string literal naming the tool, which SURVIVES migration (smoke_find_uses.mjs:103)",
      "  log('Opening find_uses_smoke fixture via open_file (with retry for WebView init)…');",
    ],
    [
      "the compliant call itself (smoke_find_uses.mjs:104)",
      "  await openFileWithRetry(rpc, FIXTURE, { fail });",
    ],
    ["an unrelated rpc call", "  const store = await rpc('store_state');"],
  ])("does not flag %s", (_form, source) => {
    expect(findSmokeDriverConventionViolations(source)).toEqual([]);
  });

  it("does not flag a full example call written inside a block comment", () => {
    // The DISCRIMINATING case for comment stripping: this one carries the exact
    // call shape the predicate matches, so it passes only if comments are
    // stripped BEFORE the match rather than the regex being narrowed around
    // them. sharedModuleLoad.ts:64-68 prescribes stripping over narrowing for
    // precisely this reason — narrowing reopens the blind spots above.
    const source = "/* e.g. await rpc('open_file', {path}) — use the helper instead */";
    expect(findSmokeDriverConventionViolations(source)).toEqual([]);
  });

  it("still flags a real call that sits below a comment mentioning the tool", () => {
    // Stripping must remove the COMMENT, not the rest of the line's neighbours:
    // an over-eager stripper that ate to end-of-source would silently disarm the
    // guard while every case above kept passing.
    const source = [
      "  // Retry open_file up to 8 times (≤45s).",
      "  openResult = await rpc('open_file', { path: FIXTURE });",
    ].join("\n");
    expect(codesFor(source)).toEqual(["inline-open-file"]);
  });

  it("reports one violation for a driver carrying the whole inline retry loop", () => {
    // The real shape being retired, end to end — a driver trips the guard ONCE,
    // naming the convention, rather than once per line that mentions the tool.
    const source = [
      "  // Retry open_file up to 8 times (≤45s) to give the WebView time.",
      "  log('Opening the fixture via open_file (with retry for WebView init)…');",
      "  let openResult = null;",
      "  for (let attempt = 1; attempt <= 8; attempt++) {",
      "    openResult = await rpc('open_file', { path: FIXTURE });",
      "    console.log(`  open_file attempt ${attempt} result:`, JSON.stringify(openResult));",
      "    if (openResult && openResult.ok) break;",
      "    if (attempt < 8) await sleep(3000);",
      "  }",
      "  if (!openResult || !openResult.ok) fail('open_file failed after retries');",
    ].join("\n");
    expect(codesFor(source)).toEqual(["inline-open-file"]);
  });

  it("carries a human-readable message alongside the code", () => {
    // The code is the contract; this is the one place the prose is pinned at
    // all, so rewording it stays a one-line edit rather than an N-assertion churn.
    expect(findSmokeDriverConventionViolations("await rpc('open_file', {});")).toEqual([
      { code: "inline-open-file", message: expect.stringContaining("openFileWithRetry") },
    ]);
  });
});
