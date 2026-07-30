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
 *
 * Violation assertions are on `code`, never on `message`: the prose is meant to
 * stay free to reword or enrich without churning the pins.
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { describe, it, expect } from "vitest";

import {
  SHARED_ESM_MODULES,
  discoverSharedEsmModules,
  findBareNodeLoadViolations,
  readSharedModuleSource,
} from "./sharedModuleLoad.js";

/** The violation identities a source trips, in the order the predicate reports them. */
function codesFor(source: string): string[] {
  return findBareNodeLoadViolations(source).map((violation) => violation.code);
}

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
    expect(codesFor(source)).toContain("node-builtin");
  });

  it.each([
    ["a CommonJS require of a builtin", 'const fs = require("node:fs");'],
    ["a CommonJS require with a space before the paren", 'const x = require ("./x");'],
  ])("flags %s", (_form, source) => {
    expect(codesFor(source)).toContain("commonjs-require");
  });

  it("reports BOTH violations when a source trips both constraints", () => {
    // Asserting on the list rather than on a boolean is what makes a failure name
    // which constraint broke instead of just saying the module is bad.
    const source = 'import "node:fs";\nconst x = require("./x");';
    expect(codesFor(source)).toEqual(["node-builtin", "commonjs-require"]);
  });

  it("carries a human-readable message alongside each code", () => {
    // The codes are the contract; this is the one place the prose is pinned at
    // all, so rewording it stays a one-line edit rather than an 8-assertion churn.
    expect(findBareNodeLoadViolations('import "node:fs";\nrequire("./x");')).toEqual([
      { code: "node-builtin", message: expect.stringContaining("node:") },
      { code: "commonjs-require", message: expect.stringContaining("require(") },
    ]);
  });
});

describe("discoverSharedEsmModules — the filter rule behind the completeness guard", () => {
  it("keeps shared `.mjs` modules, drops `smoke_*` drivers and non-`.mjs` files", () => {
    // Driven off a fixture directory so the RULE is pinned in isolation. The
    // opt-out prefix is `smoke_` WITH the underscore, so `smokeDriverGuards.mjs`
    // is a shared module while `smoke_appearance_e2e.mjs` is a driver — a
    // regression from `startsWith("smoke_")` to `startsWith("smoke")` would
    // otherwise surface only as a confusing table mismatch in the guard below,
    // never naming the filter as the thing that broke.
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "reify-shared-esm-"));
    try {
      for (const name of [
        "rpcEnvelope.mjs", // a shared module
        "smokeDriverGuards.mjs", // `smoke` but NOT `smoke_` — still shared
        "smoke_appearance_e2e.mjs", // a driver: opted out by name
        "sharedModuleLoad.ts", // not a `.mjs` at all
        "notes.md",
      ]) {
        fs.writeFileSync(path.join(dir, name), "");
      }
      expect(discoverSharedEsmModules(dir).sort()).toEqual([
        "rpcEnvelope.mjs",
        "smokeDriverGuards.mjs",
      ]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
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
    // Rendering the messages (not the codes) is the suite's job — the helper
    // stays vitest-free, and a failure here should read as prose naming which
    // constraint broke on which module.
    const violations = findBareNodeLoadViolations(readSharedModuleSource(name));
    expect(
      violations.map((violation) => violation.message),
      `${name} must stay loadable by bare \`node\``,
    ).toEqual([]);
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

  it("covers every `.mjs` in the directory that has not opted out by name", () => {
    // The one behaviour the hoist ADDS beyond de-duplication. Giving the
    // constraint a single home halves the drift surface but does not remove it:
    // a hand-maintained table still silently skips a NEW shared module until
    // someone remembers to add it. Reading the directory closes that.
    //
    // Both directions on purpose — it also catches a STALE entry left after a
    // module is deleted or renamed, which would otherwise surface as a confusing
    // ENOENT deep inside readSharedModuleSource.
    //
    // RESIDUAL HOLE, deliberately not closed here: the opt-out is a filename
    // prefix with no signal on misuse, so a genuinely shared library module
    // named `smoke_helpers.mjs` is dropped by discovery, keeps the table
    // consistent, passes this guard, and is never checked. What this catches is
    // only the opt-IN direction — a non-`smoke_` module missing from the table.
    // Closing the other direction means classifying on content (say, a `.mjs`
    // with no top-level side effects that a `.test.ts` imports) rather than on
    // the prefix; the retired copies had the same hole with a hardcoded list.
    expect(discoverSharedEsmModules().sort()).toEqual([...SHARED_ESM_MODULES].sort());
  });
});
