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

// ─── Assertion type + evaluateAssertion ──────────────────────────────────────

export type Assertion = {
  path: string;
  op: "equals" | "atLeast" | "exists";
  expected?: unknown;
};

type AssertionResult = { ok: true } | { ok: false; message: string };

/**
 * Evaluate a single declarative assertion against a value.
 *
 * - 'equals': deep comparison via JSON.stringify (structural equality)
 * - 'atLeast': actual must be a number >= Number(expected)
 * - 'exists': actual must not be undefined
 *
 * Failure message always includes the path plus expected vs actual.
 */
export function evaluateAssertion(value: unknown, a: Assertion): AssertionResult {
  const actual = getByPath(value, a.path);

  switch (a.op) {
    case "equals": {
      const ok = JSON.stringify(actual) === JSON.stringify(a.expected);
      if (ok) return { ok: true };
      return {
        ok: false,
        message: `${a.path}: expected equals ${JSON.stringify(a.expected)}, got ${JSON.stringify(actual)}`,
      };
    }
    case "atLeast": {
      if (typeof actual === "number" && actual >= Number(a.expected)) {
        return { ok: true };
      }
      return {
        ok: false,
        message: `${a.path}: expected atLeast ${String(a.expected)}, got ${JSON.stringify(actual)}`,
      };
    }
    case "exists": {
      if (actual !== undefined) return { ok: true };
      return { ok: false, message: `${a.path}: expected exists, got undefined` };
    }
  }
}
