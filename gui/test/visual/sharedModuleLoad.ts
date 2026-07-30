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
 * This module is vitest-free, like every other helper in this directory
 * (assertions.ts, diff.ts, paths.ts, rpc.ts): it exposes a pure predicate and a
 * plain read, and every `expect` lives in `./sharedModuleLoad.test.ts`. The
 * predicate doing no I/O is what lets each reference form be pinned from a
 * string literal with no on-disk fixture.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * The stable identity of a violation. Assert on THIS, never on the message.
 *
 * `node-builtin` — a `node:` builtin is referenced in any form.
 * `commonjs-require` — a CommonJS `require(` call, which ESM has no such thing as.
 */
export type BareNodeViolationCode = "node-builtin" | "commonjs-require";

/**
 * One reason a source would fail to load under a bare `node <driver>.mjs`.
 *
 * The split exists so the two halves can evolve independently: `code` is the
 * contract the suite asserts on, leaving `message` free to be reworded or
 * enriched — with the offending line number, say — without churning a single
 * assertion.
 */
export interface BareNodeViolation {
  readonly code: BareNodeViolationCode;
  readonly message: string;
}

/**
 * Every way `source` would fail to load under a bare `node <driver>.mjs`.
 *
 * Returns a list; `[]` means loadable. A LIST rather than a boolean on purpose:
 * the assertion failure then names which constraint broke, instead of dumping
 * the whole source blob.
 *
 * SCOPE — this matches the WHOLE source, comments and string literals included;
 * there is no parse. A shared module that merely *documents* the constraint
 * therefore trips it, and the message will point at an import that does not
 * exist: writing `import "node:fs"` in a doc example, or the word `require(` in
 * prose, is enough. `./rpcEnvelope.mjs`'s header discusses `node:` imports at
 * length and survives only because it writes them in backticks rather than
 * quotes. If that false positive ever bites, strip line and block comments
 * before matching — do NOT narrow the regexes back toward the `from`-anchored
 * form this file deliberately retired, which would reopen the side-effect and
 * dynamic-import blind spots the negative controls exist to pin.
 */
export function findBareNodeLoadViolations(source: string): BareNodeViolation[] {
  const violations: BareNodeViolation[] = [];
  if (/["']node:/.test(source)) {
    violations.push({ code: "node-builtin", message: "references a `node:` builtin" });
  }
  if (/require\s*\(/.test(source)) {
    violations.push({ code: "commonjs-require", message: "uses CommonJS `require(`" });
  }
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
 * Every `.mjs` in `dir` that is NOT a `smoke_*` driver — i.e. every module the
 * constraint applies to, derived rather than remembered.
 *
 * Deliberately used ONLY by the completeness guard, never as the source for the
 * `it.each` table: driving the table off a directory read would let a discovery
 * bug (wrong `dir`, a filter typo) collapse it to zero registered tests and a
 * silently green suite. {@link SHARED_ESM_MODULES} stays the table; this only
 * cross-checks it.
 *
 * The `dir` parameter exists so that filter rule can be pinned against a fixture
 * directory in isolation. The subtlety worth pinning: the opt-out is the prefix
 * `smoke_` WITH the underscore, so `smokeDriverGuards.mjs` is a shared module and
 * `smoke_appearance_e2e.mjs` is a driver.
 */
export function discoverSharedEsmModules(dir: string = VISUAL_DIR): string[] {
  return fs.readdirSync(dir).filter((name) => name.endsWith(".mjs") && !name.startsWith("smoke_"));
}

/**
 * Read the named module out of {@link VISUAL_DIR}, for checking with
 * {@link findBareNodeLoadViolations}.
 *
 * Split from the assertion so this module stays vitest-free; the `expect` lives
 * in `./sharedModuleLoad.test.ts`, which is also where the violation messages get
 * rendered into the failure output.
 */
export function readSharedModuleSource(name: string): string {
  return fs.readFileSync(path.join(VISUAL_DIR, name), "utf8");
}
