/**
 * The ToolDef-name extraction over gui/src-tauri/src/debug_server.rs —
 * one home, hoisted here from two drifting copies (task 6065).
 *
 * WHY THIS MODULE EXISTS.  `./debugParity.test.ts` (the cross-language parity
 * guard, task-4348) and `gui/test/visual/assertions.test.ts` (the
 * KNOWN_DEBUG_TOOL_NAMES parity guard, task-5934) each held their own copy of
 * the debug_server.rs read plus a byte-identical extraction regex.  Two copies
 * of one regex is two places to keep in sync; both now import from here.
 *
 * WHERE IT LIVES.  gui/tsconfig.json is `include: ["src"]`, so a module under
 * gui/src/__tests__/ is inside tsc's strict program while one under
 * gui/test/visual/ would not be.  Colocating a non-test helper in
 * gui/src/__tests__/ follows the existing precedent there
 * (debugBridgeTestHelpers.ts, bridgeMockContract.ts); vitest's default include
 * only collects `*.{test,spec}.*`, so this file is not picked up as a suite.
 *
 * This module is vitest-free, like the shared helpers under gui/test/visual/
 * (assertions.ts, diff.ts, paths.ts, rpc.ts, sharedModuleLoad.ts): it exposes
 * pure functions and a plain read, and every `expect` lives in the importing
 * `.test.ts` files.  The pure functions taking a source STRING rather than
 * doing their own I/O is what lets ./toolDefNames.test.ts pin every form from a
 * string literal with no on-disk fixture.
 */
import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Extract the tool names advertised by `tool_defs()` in debug_server.rs.
 *
 * The regex is carried over verbatim from debugParity.test.ts, the longest
 * standing copy, so both importers keep exactly the semantics they already
 * depend on:
 *
 *   - `\s` spans the newline between `ToolDef {` and `name:`, which is how
 *     every literal in debug_server.rs is actually written.
 *   - `struct ToolDef {`'s own field `name: &'static str` is UNQUOTED, so it is
 *     never captured — only string-literal name fields are.  That is not a
 *     happy accident but the identity behind `analyzeToolDefs`' `-1`.
 *   - A name containing characters outside [a-z0-9_] (e.g. uppercase or a
 *     hyphen) is DROPPED rather than captured.  The raw `ToolDef {` count in
 *     `analyzeToolDefs` still includes it, so such drift surfaces as a count
 *     mismatch instead of being masked behind a `>=` floor.
 *
 * Names are returned in source order, duplicates included.
 */
export function extractToolDefNames(rustSource: string): string[] {
  return [...rustSource.matchAll(/ToolDef\s*\{\s*name:\s*"([a-z0-9_]+)"/g)].map((m) => m[1]);
}

/** The extraction plus the cross-checks a caller needs to detect drift. */
export interface ToolDefExtraction {
  /** Captured tool names, in source order, duplicates included. */
  names: string[];
  /** Every `ToolDef {` occurrence, including the `struct ToolDef {` definition. */
  rawToolDefCount: number;
  /** `rawToolDefCount - 1` — the one non-literal occurrence is the struct definition. */
  expectedNameCount: number;
  /** Each name appearing more than once, listed once, in first-repeat order. */
  duplicates: string[];
}

/**
 * Run `extractToolDefNames` and pair it with the two independent cross-checks.
 *
 * `names.length === expectedNameCount` catches a name the regex could not
 * capture; `duplicates` catches a name captured twice.  Neither subsumes the
 * other: a duplicate keeps the count correct, and a dropped name leaves the
 * names unique.  Returning `duplicates` as a LIST rather than comparing set
 * sizes is what lets a failing assertion NAME the offending tool instead of
 * reporting a bare count mismatch.
 */
export function analyzeToolDefs(rustSource: string): ToolDefExtraction {
  const names = extractToolDefNames(rustSource);
  const rawToolDefCount = rustSource.match(/ToolDef\s*\{/g)?.length ?? 0;

  const seen = new Set<string>();
  const duplicates: string[] = [];
  for (const name of names) {
    if (seen.has(name)) {
      if (!duplicates.includes(name)) duplicates.push(name);
    } else {
      seen.add(name);
    }
  }

  return {
    names,
    rawToolDefCount,
    expectedNameCount: rawToolDefCount - 1,
    duplicates,
  };
}

/**
 * Read gui/src-tauri/src/debug_server.rs.
 *
 * The path is resolved from THIS module's own location, not the caller's:
 * `import.meta.url` is per-module, so a single computation is correct for every
 * importer regardless of which directory it sits in.  That is what replaced the
 * two different path computations the copies used
 * (`join(__dirname, '../../src-tauri/...')` in one, `resolveRepoRoot()` +
 * `path.join` in the other).
 *
 * The `fileURLToPath` + `path.dirname` + `path.resolve` idiom is the one proven
 * in gui/test/visual/paths.ts, which documents why naive URL-relative `..` math
 * over-shoots.  Segment count here:
 *
 *   <gui>/src/__tests__/toolDefNames.ts
 *              ^^^^^^^^   (dirname = __tests__/)
 *         ^^^               (..     = src/)
 *   ^^^^^                   (..     = gui/)
 */
export function readDebugServerSource(): string {
  const dir = path.dirname(fileURLToPath(import.meta.url));
  return readFileSync(path.resolve(dir, '..', '..', 'src-tauri', 'src', 'debug_server.rs'), 'utf-8');
}
