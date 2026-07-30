/**
 * The bare-`node` load constraint on this directory's shared `.mjs` modules —
 * one home, hoisted here from two drifting copies (task 5859).
 *
 * WHY THE CONSTRAINT EXISTS. The six smoke runners invoke `node <driver>.mjs`
 * and never `tsx`, while vitest loads those same shared modules through vite's
 * browser-condition resolver. A `node:` reference therefore resolves fine for
 * the drivers and breaks under vitest — a split that only shows up at the next
 * live run. Vitest cannot catch it by merely importing the module either,
 * because the checking file itself imports `node:fs` quite happily. A
 * source-level check is the only way the constraint fails loudly.
 *
 * WHY IT MATCHES ON THE QUOTE. `/["']node:/` rather than `/from\s+["']node:/`
 * covers every reference form: `import x from "node:fs"`, a side-effect
 * `import "node:fs"`, the no-space `from"node:fs"`, and a dynamic
 * `await import("node:fs")`. The narrower `from`-anchored regex that used to
 * live in `./rpcEnvelope.test.ts` saw only the first two; this file keeps the
 * broader one, and `./sharedModuleLoad.test.ts` pins each form so a silent
 * regression back to the narrow reading cannot happen.
 *
 * The predicate below is PURE — no I/O, no vitest — which is what lets those
 * forms be tested from string literals with no on-disk fixture. Importing
 * `expect` in a non-`.test.ts` helper is a deliberate, isolated divergence from
 * this directory's pure-module convention (assertions.ts, diff.ts, paths.ts,
 * rpc.ts): it is confined to the thin `expectBareNodeLoadable` wrapper, and the
 * regex logic itself stays vitest-free and unit-testable.
 */
import { expect } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Every way `source` would fail to load under a bare `node <driver>.mjs`.
 *
 * Returns a human-readable list; `[]` means loadable. A LIST rather than a
 * boolean on purpose: the assertion failure then names which constraint broke,
 * instead of dumping the whole source blob.
 */
export function findBareNodeLoadViolations(source: string): string[] {
  const violations: string[] = [];
  if (/["']node:/.test(source)) violations.push("references a `node:` builtin");
  if (/require\s*\(/.test(source)) violations.push("uses CommonJS `require(`");
  return violations;
}

/**
 * The directory holding the shared `.mjs` modules and the `smoke_*.mjs` drivers.
 *
 * `import.meta.url` resolves to this file's own source path under vite's
 * transform — the same idiom both retired copies used, and this file sits in the
 * same directory, so the resolved path is unchanged.
 */
export const VISUAL_DIR = path.dirname(fileURLToPath(import.meta.url));

/**
 * The SHARED library modules only — the ones loaded by BOTH consumer families.
 *
 * `smoke_*.mjs` drivers are excluded on purpose: they are node-only entry points
 * that legitimately import `node:path` / `node:url`.
 */
export const SHARED_ESM_MODULES = [
  "rpcEnvelope.mjs",
  "meshCountParity.mjs",
  "smokeDriverGuards.mjs",
];

/**
 * Every `.mjs` in this directory that is NOT a `smoke_*` driver — i.e. every
 * module the constraint applies to, derived rather than remembered.
 *
 * Deliberately used ONLY by the completeness guard, never as the source for the
 * `it.each` table: driving the table off a directory read would let a discovery
 * bug (wrong `dir`, a filter typo) collapse it to zero registered tests and a
 * silently green suite. {@link SHARED_ESM_MODULES} stays the table; this only
 * cross-checks it.
 */
export function discoverSharedEsmModules(dir: string = VISUAL_DIR): string[] {
  return fs.readdirSync(dir).filter((name) => name.endsWith(".mjs") && !name.startsWith("smoke_"));
}

/** Assert that the named module in {@link VISUAL_DIR} stays bare-`node` loadable. */
export function expectBareNodeLoadable(name: string): void {
  const source = fs.readFileSync(path.join(VISUAL_DIR, name), "utf8");
  expect(
    findBareNodeLoadViolations(source),
    `${name} must stay loadable by bare \`node\``,
  ).toEqual([]);
}
