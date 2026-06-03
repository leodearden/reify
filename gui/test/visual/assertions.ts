/**
 * Value-assertion harness for the reify-debug e2e test suite.
 *
 * Pure module — no I/O. All integration glue lives in run.ts.
 * Mirrors the established pure-module convention (diff.ts, rpc.ts, paths.ts).
 */

// ─── getByPath ────────────────────────────────────────────────────────────────

/**
 * Resolve a dotted path against an arbitrary value.
 * Returns `undefined` on any missing or non-object segment, never throws.
 *
 * Example: getByPath({ engine: { meshCount: 1 } }, "engine.meshCount") === 1
 */
export function getByPath(obj: unknown, dotted: string): unknown {
  const segments = dotted.split(".");
  let current: unknown = obj;
  for (const seg of segments) {
    if (current === null || current === undefined || typeof current !== "object") {
      return undefined;
    }
    current = (current as Record<string, unknown>)[seg];
  }
  return current;
}
