/**
 * CI cover for `./smokeDriverConventions.ts` — the structural conventions the
 * live `smoke_*.mjs` drivers must follow: today, the `open_file` retry policy
 * living in one home (task 5857) and the post-open `store_state` payload being
 * diagnosed before it is read (task 5884).
 *
 * Read that module's header for WHY a source-level check is the only executable
 * signal available here: a driver needs a live reify-gui (WebKit WebView +
 * OCCT), so CI can never run one and `node --check` catches syntax alone.
 *
 * The negative controls below are the load-bearing half. A source-level regex
 * check is INERT BY DEFAULT — a regex that matches too much reports an
 * ALREADY-COMPLIANT driver as a violator, and a regex that matches too little
 * reports nobody at all; either way the constraint stops meaning what it says.
 * So each false-positive shape gets its own case, one per mechanism:
 *   - `open_file` appears in prose comments in all six drivers AND inside a
 *     string literal that SURVIVES migration.
 *   - the `store_state` check adds two shapes of its own, because it is a
 *     NEGATIVE-presence check ("reads X and lacks Y") and so has a false-alarm
 *     mechanism the first one does not: a driver carrying the compliant guard,
 *     and a driver that never binds `storeAfterOpen` at all and therefore has no
 *     read to diagnose. Its raw-vs-stripped asymmetry — the thing that keeps
 *     those from false-alarming — is pinned in BOTH directions, so a later
 *     "make it consistent" edit flips a case instead of reversing an argument.
 *
 * Violation assertions are on `code`, never on `message`: the prose is meant to
 * stay free to reword or enrich without churning the pins.
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { describe, it, expect } from "vitest";

import { readSharedModuleSource } from "./sharedModuleLoad.js";
import {
  SMOKE_DRIVERS,
  discoverSmokeDrivers,
  findSmokeDriverConventionViolations,
  stripComments,
} from "./smokeDriverConventions.js";

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

describe("findSmokeDriverConventionViolations — the post-open `store_state` diagnosis", () => {
  // One case per spelling of the UNGUARDED read. The optional chain is the whole
  // defect: an in-band `{error: '<msg>'}` is TRUTHY, so `!storeAfterOpen?.…`
  // reads past it to `undefined` and the driver reports `got: undefined` —
  // blaming the frontend for a `store_state` outage.
  it.each([
    [
      "the shape smoke_mesh_count_parity_e2e.mjs:200-205 carries today",
      [
        "  const storeAfterOpen = await rpc('store_state');",
        "  if (!storeAfterOpen?.editor?.activeFile?.includes('large_assembly')) {",
        "    fail(",
        "      `Expected activeFile to contain 'large_assembly', got: ${storeAfterOpen?.editor?.activeFile}`,",
        "    );",
        "  }",
      ].join("\n"),
    ],
    [
      "a whitespace variant a formatter could introduce at any time",
      "  if (!storeAfterOpen ?. editor ?. activeFile) fail('nope');",
    ],
    [
      "the read alone, with no `store_state` call in the snippet",
      "  const active = storeAfterOpen?.editor?.activeFile;",
    ],
  ])("flags %s", (_form, source) => {
    expect(codesFor(source)).toEqual(["undiagnosed-store-state"]);
  });

  // THE FALSE POSITIVES — the load-bearing half again. Each of these is a shape
  // an ALREADY-COMPLIANT driver carries, so a predicate that flags any of them
  // fails the corpus guard for the wrong reason and gets "fixed" by editing a
  // correct driver.
  it.each([
    [
      "the compliant shape verbatim (smoke_multi_pane_e2e.mjs:128-136)",
      [
        "  const storeAfterOpen = await rpc('store_state');",
        "  // Diagnose the RPC before the optional chain reads past an in-band error —",
        "  // see ./smokeDriverGuards.mjs (describeRpcFailure).",
        "  const storeAfterOpenFailure = describeRpcFailure(storeAfterOpen, 'store_state (post-open)');",
        "  if (storeAfterOpenFailure) fail(storeAfterOpenFailure);",
        "  if (!storeAfterOpen?.editor?.activeFile?.includes('multi_pane_viewport')) {",
        "    fail(`Expected activeFile to contain 'multi_pane_viewport', got: ${storeAfterOpen?.editor?.activeFile}`);",
        "  }",
      ].join("\n"),
    ],
    [
      "the chain named in a line comment only",
      "  // storeAfterOpen?.editor?.activeFile is read below, once diagnosed.",
    ],
    [
      "the chain named in a block comment only",
      "  /* e.g. storeAfterOpen?.editor?.activeFile — diagnose the payload first */",
    ],
    [
      "a driver that never binds `storeAfterOpen` (smoke_diagnostics_e2e.mjs / smoke_find_uses.mjs)",
      [
        "  await openFileWithRetry(rpc, FIXTURE, { fail });",
        "  const diagnostics = await rpc('get_diagnostics');",
        "  if (!diagnostics?.diagnostics?.length) fail('expected diagnostics');",
      ].join("\n"),
    ],
  ])("does not flag %s", (_form, source) => {
    expect(findSmokeDriverConventionViolations(source)).toEqual([]);
  });

  it("does not flag a plain-dot read of the same payload", () => {
    // A DELIBERATE non-violation, pinned by name so it stays deliberate. This
    // convention defends against SILENCE, not against every unguarded read: on
    // an `{error: '<msg>'}` payload the plain-dot form yields `undefined` at
    // `.editor` and then THROWS a TypeError at `.activeFile` — a stack trace
    // naming the exact line, already loud and already attributable. Only the
    // optional chain reads past the error to a quiet `got: undefined`.
    const source = [
      "  const storeAfterOpen = await rpc('store_state');",
      "  if (!storeAfterOpen.editor.activeFile.includes('large_assembly')) fail('nope');",
    ].join("\n");
    expect(findSmokeDriverConventionViolations(source)).toEqual([]);
  });

  it("reads the diagnosis off the RAW source, so a blind spot cannot false-alarm", () => {
    // DIRECTION 1 of the raw-vs-stripped asymmetry. The `//`-containing-`/*`
    // blind spot pinned below opens a block span that blanks lines 1-3 here —
    // swallowing the REAL diagnosis while leaving the read on line 4 intact. Were
    // DIAG matched against the stripped code, `lacks DIAG` would be true and this
    // COMPLIANT driver would be reported as a violator: a false alarm, which the
    // module's header calls strictly worse because it gets fixed by editing
    // correct code. Matching DIAG on the raw source puts the blind spot out of
    // reach for this half. A later "make it consistent, strip for both" cleanup
    // flips THIS case rather than silently reversing the acceptance argument.
    const source = [
      "  // see /* the helper for the real thing",
      "  const storeAfterOpenFailure = describeRpcFailure(storeAfterOpen, 'store_state (post-open)');",
      "  if (storeAfterOpenFailure) fail(storeAfterOpenFailure); /* end */",
      "  if (!storeAfterOpen?.editor?.activeFile?.includes('large_assembly')) fail('nope');",
    ].join("\n");
    expect(findSmokeDriverConventionViolations(source)).toEqual([]);
    // …and the SAME source with the diagnosis genuinely gone IS flagged. Without
    // this counter-assertion the case above would pass for a predicate that
    // never fires at all — the inert-by-default failure this file exists to stop.
    expect(
      codesFor(source.replace("describeRpcFailure(storeAfterOpen", "JSON.stringify(storeAfterOpen")),
    ).toEqual(["undiagnosed-store-state"]);
  });

  it("lets a diagnosis written only inside a comment suppress the flag", () => {
    // DIRECTION 2, and the PRICE of direction 1. Reading DIAG off the raw source
    // means a `describeRpcFailure(storeAfterOpen, …)` that exists only as prose
    // silences a genuinely unguarded read.
    //
    // Asserted as the CURRENT behaviour, not as desirable: it is accepted only
    // because it errs toward a MISSED violation — one driver stays unmigrated
    // until someone reads it — which is the direction this module already
    // accepts everywhere else. Both halves obey one rule: each is matched
    // against whichever text errs toward silence.
    const source = [
      "  // Diagnose with describeRpcFailure(storeAfterOpen, 'store_state (post-open)') first.",
      "  if (!storeAfterOpen?.editor?.activeFile?.includes('large_assembly')) fail('nope');",
    ].join("\n");
    expect(findSmokeDriverConventionViolations(source)).toEqual([]);
    // The discriminating half again: drop the mention and the read IS flagged.
    expect(codesFor(source.replace("describeRpcFailure(storeAfterOpen, ", "the helper on "))).toEqual(
      ["undiagnosed-store-state"],
    );
  });

  it("carries a human-readable message alongside the code", () => {
    // As above, the code is the contract and this is the ONE place this
    // convention's prose is pinned, so rewording stays a one-line edit.
    expect(findSmokeDriverConventionViolations("storeAfterOpen?.editor?.activeFile;")).toEqual([
      {
        code: "undiagnosed-store-state",
        message: expect.stringContaining("describeRpcFailure"),
      },
    ]);
  });

  it("reports each convention exactly once for a driver tripping both", () => {
    // The at-most-once contract, pinned across the WIDENED predicate: a driver
    // with two problems gets two violations, not one per offending line.
    const source = [
      "  openResult = await rpc('open_file', { path: FIXTURE });",
      "  console.log(`  open_file result:`, JSON.stringify(openResult));",
      "  const storeAfterOpen = await rpc('store_state');",
      "  if (!storeAfterOpen?.editor?.activeFile?.includes('large_assembly')) fail('nope');",
      "  log(`active: ${storeAfterOpen?.editor?.activeFile}`);",
    ].join("\n");
    expect(codesFor(source)).toEqual(["inline-open-file", "undiagnosed-store-state"]);
  });
});

describe("stripComments — the mitigation the predicate is built on", () => {
  // Exported and therefore pinned directly, not only through the predicate: its
  // header makes two claims a caller could rely on, and both are load-bearing
  // for the "future message reports a line number" affordance it promises.

  it("preserves the line count across a multi-line block comment", () => {
    // Blanking a block span to spaces rather than deleting it is what keeps
    // ORIGINAL line numbers addressable. A `.replace(…, "")` here would still
    // pass every predicate case above while quietly making that claim false.
    const source = ["/* a", " b", " c */", "await rpc('open_file', {});"].join("\n");
    expect(stripComments(source).split("\n")).toHaveLength(4);
    expect(codesFor(source)).toEqual(["inline-open-file"]);
  });

  it("preserves the line count across a line comment", () => {
    // The `//` pass deletes to end-of-line rather than blanking, so columns do
    // NOT survive it — but the newline itself must, or every line number below
    // a comment shifts.
    const source = ["// leading note", "await rpc('open_file', {});", "// trailing note"].join("\n");
    expect(stripComments(source).split("\n")).toHaveLength(3);
  });

  it("lets a `//` comment containing `/*` swallow the code below it", () => {
    // A KNOWN BLIND SPOT, pinned so it stays a known one. The block pass runs
    // FIRST, so `/*` inside a line comment opens a span that runs to the next
    // `*/` anywhere below and blanks the real call in between.
    //
    // Asserted as the CURRENT behaviour, not as desirable: it is accepted only
    // because it errs toward a MISSED violation. Should a later edit reverse
    // that asymmetry — making some compliant driver a false alarm — this case
    // flips and forces the trade-off to be re-argued rather than re-discovered.
    const source = [
      "// see /* the helper for the real thing",
      "openResult = await rpc('open_file', { path: FIXTURE });",
      "const done = 1; /* trailing */",
    ].join("\n");
    expect(findSmokeDriverConventionViolations(source)).toEqual([]);
    // …and the same source with the stray `/*` removed IS flagged, which is what
    // makes this a statement about the ordering rather than about the regex.
    expect(codesFor(source.replace("/* the helper", "the helper"))).toEqual(["inline-open-file"]);
  });
});

describe("discoverSmokeDrivers — the delegation to the shared split rule", () => {
  it("returns the driver half of the partition", () => {
    // The split RULE lives in ./sharedModuleLoad.ts's partitionVisualMjs and is
    // pinned once, there. This pins only the delegation: were it to regress to
    // the `shared` half, the sole failure would be the completeness guard below
    // reporting a SMOKE_DRIVERS table mismatch that never names the filter as
    // the thing that broke.
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "reify-smoke-drivers-"));
    try {
      for (const name of [
        "smoke_find_uses.mjs", // a driver
        "smokeDriverGuards.mjs", // `smoke` but NOT `smoke_` — shared, not a driver
        "rpcEnvelope.mjs", // a shared module
        "x.ts", // not a `.mjs` at all
      ]) {
        fs.writeFileSync(path.join(dir, name), "");
      }
      expect(discoverSmokeDrivers(dir)).toEqual(["smoke_find_uses.mjs"]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("the conventions the live drivers are held to, across the whole corpus", () => {
  // THE POINT OF THIS FILE, for each convention in turn. Each driver
  // re-implementing the 8×3000ms loop was its own copy of the retry budget, the
  // `.ok` verdict and the failure wording — and each inline copy silently lacks
  // the helper's transport-error folding, so an ECONNREFUSED during the WebView
  // boot window aborts the whole budget on attempt 1 and lands in main().catch
  // as exit 2 instead of the clean exit-1 verdict. Each unguarded post-open
  // `store_state` read, in turn, reports `activeFile: undefined` when the RPC
  // itself failed, sending whoever reads the run at the frontend instead of at
  // the tool. Nothing else in CI can observe either: these drivers need a live GUI.
  it.each(SMOKE_DRIVERS)("%s breaks none of the driver conventions", (name) => {
    // Rendering the messages (not the codes) is the suite's job — the helper
    // stays vitest-free, and a failure here should read as prose naming which
    // convention broke on which driver.
    //
    // The hint stays convention-NEUTRAL on purpose. Naming one convention here
    // misattributes every other: a RED `undiagnosed-store-state` under an
    // "open_file retry policy" hint sends an operator to entirely the wrong
    // subsystem. The `message` above already names the right one — the same
    // code-is-the-contract / message-is-free split ./smokeDriverConventions.ts
    // documents.
    const violations = findSmokeDriverConventionViolations(readSharedModuleSource(name));
    expect(
      violations.map((violation) => violation.message),
      `${name} must follow the conventions in ./smokeDriverConventions.ts (see the message above)`,
    ).toEqual([]);
  });

  it("names the six known drivers — an empty table is not a pass", () => {
    // Without this, an edit that empties the table turns the `it.each` above
    // into zero registered tests: a vacuously green suite with the convention
    // silently gone, the same inert-by-default failure the negative controls
    // guard against on the regex side.
    expect(SMOKE_DRIVERS).toEqual(
      expect.arrayContaining([
        "smoke_appearance_e2e.mjs",
        "smoke_diagnostics_e2e.mjs",
        "smoke_find_uses.mjs",
        "smoke_mesh_count_parity_e2e.mjs",
        "smoke_multi_pane_e2e.mjs",
        "smoke_surface_finish_viewport_e2e.mjs",
      ]),
    );
  });

  it("covers every `smoke_*.mjs` driver in the directory", () => {
    // A hand-maintained table silently skips a NEW driver until someone
    // remembers to add it — which is precisely how a seventh inline copy of the
    // retry loop would get in. Reading the directory closes that.
    //
    // Both directions on purpose, so it also catches a STALE entry left after a
    // driver is deleted or renamed, which would otherwise surface as a confusing
    // ENOENT deep inside readSharedModuleSource.
    expect(discoverSmokeDrivers().sort()).toEqual([...SMOKE_DRIVERS].sort());
  });
});
