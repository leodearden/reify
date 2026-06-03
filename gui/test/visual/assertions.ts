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

// ─── FIXTURES catalogue ───────────────────────────────────────────────────────

/**
 * Named fixture catalogue — name → repo-relative path.
 *
 * Values match the existing Scenario.fixture convention in run.ts so the same
 * `path.join(REPO_ROOT, rel)` plumbing resolves both visual and value fixtures.
 */
export const FIXTURES = {
  empty: "gui/test/fixtures/empty.ri",
  small_cube: "gui/test/fixtures/small_cube.ri",
  broken_syntax: "gui/test/fixtures/broken_syntax.ri",
  large_assembly: "gui/test/fixtures/large_assembly.ri",
  all_severities: "gui/test/fixtures/all_severities.ri",
} as const;

// ─── ValueScenario type + VALUE_SCENARIOS catalogue ──────────────────────────

/**
 * A declarative value-assertion scenario: open a fixture, call a tool,
 * assert on the returned JSON.
 */
export type ValueScenario = {
  /** Unique identifier for the scenario */
  name: string;
  /** Key into FIXTURES — the .ri file to open before calling the tool */
  fixture: keyof typeof FIXTURES;
  /** MCP tool name to call (e.g. "store_state") */
  tool: string;
  /** Arguments to pass to the tool */
  args: Record<string, unknown>;
  /** Assertions to evaluate against the tool's returned JSON value */
  assertions: Assertion[];
};

/**
 * Catalogue of value-assertion scenarios.
 *
 * Primary scenario: open small_cube → call store_state → assert engine.meshCount === 1.
 * Additional scenarios for other fixtures will be added by downstream tool-leaf tasks.
 */
export const VALUE_SCENARIOS: ValueScenario[] = [
  {
    name: "store_state_meshcount_small_cube",
    fixture: "small_cube",
    tool: "store_state",
    args: {},
    assertions: [{ path: "engine.meshCount", op: "equals", expected: 1 }],
  },
];

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
