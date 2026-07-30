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
 * forms be tested from string literals with no on-disk fixture.
 */

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
