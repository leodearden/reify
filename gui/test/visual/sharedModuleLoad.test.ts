/**
 * CI cover for `./sharedModuleLoad.ts` — the single home of the bare-`node` load
 * constraint on the shared `.mjs` modules (task 5859).
 *
 * Read that module's header for WHY the constraint exists at all. This suite is
 * the ONE executable pin on it: it used to be pinned twice, by a narrow copy in
 * `./rpcEnvelope.test.ts` and a broader table-driven copy in
 * `./smokeDriverGuards.test.ts`, which is exactly the drift surface this file
 * removes.
 *
 * The negative controls below are the part neither original copy had. A
 * source-level regex check is INERT BY DEFAULT: a typo'd regex or an unexpected
 * read makes every module "pass" and the constraint is silently gone. Hoisting
 * concentrates that risk into one place, so each violating reference form gets
 * its own case — a partial regression then names itself instead of going quiet.
 */
import { describe, it, expect } from "vitest";

import {
  SHARED_ESM_MODULES,
  discoverSharedEsmModules,
  expectBareNodeLoadable,
  findBareNodeLoadViolations,
} from "./sharedModuleLoad.js";

describe("findBareNodeLoadViolations — the pure predicate behind the constraint", () => {
  it("returns no violations for plain ESM that a bare-`node` driver can load", () => {
    const clean = [
      'import { isInBandError } from "./rpcEnvelope.mjs";',
      "export function describeThing(payload) {",
      '  // A node in the graph is not a `node:` builtin — the word alone is fine.',
      '  return payload === null ? "a node in the graph" : String(payload);',
      "}",
    ].join("\n");
    expect(findBareNodeLoadViolations(clean)).toEqual([]);
  });

  // One case per reference form the BROAD `/["']node:/` regex exists to catch.
  // Matching on the QUOTE rather than on `from` is what covers all of them; the
  // retired narrow copy matched `/from\s+["']node:/` and saw only the first two.
  it.each([
    ['a double-quoted named import', 'import * as fs from "node:fs";'],
    ['a single-quoted named import', "import * as fs from 'node:fs';"],
    ["a side-effect import", 'import "node:fs";'],
    ["a no-space `from\"…\"` import", 'import * as fs from"node:fs";'],
    ["a dynamic import", 'const fs = await import("node:fs");'],
  ])("flags %s", (_form, source) => {
    expect(findBareNodeLoadViolations(source)).toContain("references a `node:` builtin");
  });

  it.each([
    ["a CommonJS require of a builtin", 'const fs = require("node:fs");'],
    ["a CommonJS require with a space before the paren", 'const x = require ("./x");'],
  ])("flags %s", (_form, source) => {
    expect(findBareNodeLoadViolations(source)).toContain("uses CommonJS `require(`");
  });

  it("reports BOTH violations when a source trips both constraints", () => {
    // Asserting on the list rather than on a boolean is what makes a failure name
    // which constraint broke instead of just saying the module is bad.
    const source = 'import "node:fs";\nconst x = require("./x");';
    expect(findBareNodeLoadViolations(source)).toEqual([
      "references a `node:` builtin",
      "uses CommonJS `require(`",
    ]);
  });
});

describe("the load constraint that makes the shared driver modules .mjs", () => {
  // The executable replacement for BOTH deleted copies. The `smoke_*.mjs`
  // drivers are excluded from the table ON PURPOSE: they are node-only entry
  // points and legitimately import `node:path` / `node:url`. Only the shared
  // library modules — the ones loaded by BOTH consumer families, vitest through
  // vite's browser-condition resolver and the drivers through a bare `node` —
  // are subject to the constraint.
  it.each(SHARED_ESM_MODULES)("%s is plain ESM a bare-`node` driver can load", (name) => {
    expectBareNodeLoadable(name);
  });

  it("names the three known shared modules — an empty table is not a pass", () => {
    // Without this, an edit that empties the table turns the `it.each` above
    // into zero registered tests: a vacuously green suite with the constraint
    // silently gone, which is the same inert-by-default failure the negative
    // controls guard against on the regex side.
    expect(SHARED_ESM_MODULES).toEqual(
      expect.arrayContaining(["rpcEnvelope.mjs", "meshCountParity.mjs", "smokeDriverGuards.mjs"]),
    );
  });

  it("covers every shared `.mjs` in the directory — a new one cannot be forgotten", () => {
    // The one behaviour the hoist ADDS beyond de-duplication. Giving the
    // constraint a single home halves the drift surface but does not remove it:
    // a hand-maintained table still silently skips a NEW shared module until
    // someone remembers to add it. Reading the directory closes that.
    //
    // Both directions on purpose — it also catches a STALE entry left after a
    // module is deleted or renamed, which would otherwise surface as a confusing
    // ENOENT deep inside expectBareNodeLoadable.
    //
    // This also makes the `smoke_` prefix load-bearing rather than decorative: a
    // legitimately node-only helper added here MUST be named `smoke_*` to opt
    // out of the constraint, and a failure here says so.
    expect(discoverSharedEsmModules().sort()).toEqual([...SHARED_ESM_MODULES].sort());
  });
});
