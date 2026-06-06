/**
 * Value-assertion harness for the reify-debug e2e test suite.
 *
 * Pure module — no I/O. All integration glue lives in run.ts.
 * Mirrors the established pure-module convention (diff.ts, rpc.ts, paths.ts).
 */

import type { RpcResult } from "./rpc.js";

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
  overflow: "gui/test/fixtures/overflow.ri",
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
  {
    name: "get_window_state_devicePixelRatio",
    fixture: "small_cube",
    tool: "get_window_state",
    args: {},
    assertions: [{ path: "devicePixelRatio", op: "exists" }],
  },
  {
    name: "get_layout_metrics_overflow_clipped",
    fixture: "overflow",
    tool: "get_layout_metrics",
    args: { selector: ".cm-scroller" },
    assertions: [
      { path: "exists", op: "equals", expected: true },
      { path: "overflow.horizontal", op: "equals", expected: true },
    ],
  },
  // task-4297 step-8 GREEN: R2 e2e signal scenarios (live signal via npm run test:e2e)
  // Non-racy: openFixture in run.ts calls open_file + wait_for_idle before invoking the
  // tool, so the engine has settled and diagnostic population is complete before the
  // get_diagnostics call. broken_syntax.ri is intentionally unparseable → ≥1 compile diag.
  {
    name: "get_diagnostics_broken_syntax",
    fixture: "broken_syntax",
    tool: "get_diagnostics",
    args: {},
    assertions: [
      { path: "compile", op: "exists" },
      { path: "compileCount", op: "atLeast", expected: 1 },
    ],
  },
  {
    name: "ui_outline_small_cube",
    fixture: "small_cube",
    tool: "ui_outline",
    args: {},
    assertions: [
      { path: "outline", op: "exists" },
      { path: "count", op: "atLeast", expected: 1 },
    ],
  },
  // task-4298 step-11: R3 e2e signal scenarios (live signal via npm run test:e2e)
  // wait_for_selector: verifies the tool resolves once the main app-layout element is
  // visible. openFixture in run.ts already calls open_file + wait_for_idle before
  // invoking the tool, so the layout element is mounted and visible by this point.
  {
    name: "wait_for_selector_app_layout_visible",
    fixture: "small_cube",
    tool: "wait_for_selector",
    args: { testId: "app-layout", state: "visible" },
    assertions: [{ path: "ok", op: "equals", expected: true }],
  },
  // list_console_errors: asserts SHAPE only (errors array + count present).
  // The declarative single-tool harness cannot deterministically inject a frontend
  // JS error before the call; full message+stack signal is covered by unit tests
  // (step-1/step-3). Shape existence is sufficient as an e2e smoke check.
  {
    name: "list_console_errors_shape",
    fixture: "small_cube",
    tool: "list_console_errors",
    args: {},
    assertions: [
      { path: "errors", op: "exists" },
      { path: "count", op: "exists" },
    ],
  },
  // task-4304 F2: LSP probe e2e signal scenarios (live signal via npm run test:e2e,
  // not CI-gated — per H0 harness contract).  Positions resolved against small_cube.ri:
  //   line=7 col=10 → `size` identifier in `param size: Scalar = 10mm`
  //   line=9 col=19 → first `size` arg in `let body = box(size, size, size)`
  {
    name: "hover_at_markdown_small_cube",
    fixture: "small_cube",
    tool: "hover_at",
    // line=7, col=10: `size` parameter declaration — LSP returns hover markdown
    args: { line: 7, col: 10 },
    assertions: [{ path: "markdownLength", op: "atLeast", expected: 1 }],
  },
  {
    name: "completion_at_nonempty_small_cube",
    fixture: "small_cube",
    tool: "completion_at",
    // line=9, col=19: inside `box(size,...)` — LSP returns non-empty completion list
    args: { line: 9, col: 19 },
    assertions: [{ path: "itemCount", op: "atLeast", expected: 1 }],
  },
  {
    name: "definition_at_range_small_cube",
    fixture: "small_cube",
    tool: "definition_at",
    // line=9, col=19: `size` usage in box call → definition jumps to line 7 (param decl)
    args: { line: 9, col: 19 },
    assertions: [{ path: "range.start.line", op: "exists" }],
  },
];

// ─── Assertion type + evaluateAssertion ──────────────────────────────────────

export type Assertion = {
  path: string;
  op: "equals" | "atLeast" | "exists";
  expected?: unknown;
};

type AssertionResult = { ok: true } | { ok: false; message: string };

// Key-order-insensitive recursive deep equality. Returns true iff a and b are
// structurally equal regardless of object key insertion order.
function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    const aa = a as unknown[];
    const bb = b as unknown[];
    return aa.length === (bb as unknown[]).length && aa.every((v, i) => deepEqual(v, (bb as unknown[])[i]));
  }
  const aRec = a as Record<string, unknown>;
  const bRec = b as Record<string, unknown>;
  const aKeys = Object.keys(aRec).sort();
  const bKeys = Object.keys(bRec).sort();
  if (aKeys.length !== bKeys.length) return false;
  return aKeys.every((k, i) => k === bKeys[i] && deepEqual(aRec[k], bRec[k]));
}

/**
 * Evaluate a single declarative assertion against a value.
 *
 * - 'equals': recursive deep equality (key-order insensitive); a missing path
 *   always fails — undefined is never considered equal to any expected value.
 * - 'atLeast': actual must be a number >= Number(expected)
 * - 'exists': actual must not be undefined
 *
 * Failure message always includes the path plus expected vs actual.
 */
export function evaluateAssertion(value: unknown, a: Assertion): AssertionResult {
  const actual = getByPath(value, a.path);

  switch (a.op) {
    case "equals": {
      // Treat a missing path as an explicit failure regardless of expected,
      // preventing a false pass when expected is also omitted.
      if (actual === undefined) {
        return {
          ok: false,
          message: `${a.path}: expected equals ${JSON.stringify(a.expected)}, got undefined (path missing)`,
        };
      }
      if (deepEqual(actual, a.expected)) return { ok: true };
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
    default:
      return { ok: false, message: `unknown op: ${String(a.op)}` };
  }
}

// ─── ScenarioDeps + runValueScenario ─────────────────────────────────────────

/**
 * Injected I/O dependencies for runValueScenario.
 * Enables unit-testing scenario logic with fake deps (no live GUI).
 */
export type ScenarioDeps = {
  /** Open a fixture by repo-relative path; returns ok:true on success */
  openFixture: (repoRelPath: string) => Promise<RpcResult<unknown>>;
  /** Call a debug tool with args; returns ok:true with the JSON value */
  callTool: (tool: string, args: Record<string, unknown>) => Promise<RpcResult<unknown>>;
};

/**
 * Run a single value-assertion scenario using injected deps.
 *
 * Logic:
 * 1. Call deps.openFixture(FIXTURES[scenario.fixture]) — on failure, push an
 *    "open_file failed" message and return early (tool is NOT called).
 * 2. Call deps.callTool(scenario.tool, scenario.args) — on failure push a
 *    "<tool> failed" message.
 * 3. Evaluate each assertion via evaluateAssertion, collecting failure messages.
 * 4. Return { name, passed: failures.length===0, failures }.
 */
export async function runValueScenario(
  deps: ScenarioDeps,
  scenario: ValueScenario,
): Promise<{ name: string; passed: boolean; failures: string[] }> {
  const failures: string[] = [];

  const openResult = await deps.openFixture(FIXTURES[scenario.fixture]);
  if (!openResult.ok) {
    failures.push(`open_file failed: ${openResult.error}`);
    return { name: scenario.name, passed: false, failures };
  }

  const toolResult = await deps.callTool(scenario.tool, scenario.args);
  if (!toolResult.ok) {
    failures.push(`${scenario.tool} failed: ${toolResult.error}`);
    return { name: scenario.name, passed: false, failures };
  }

  for (const assertion of scenario.assertions) {
    const outcome = evaluateAssertion(toolResult.value, assertion);
    if (!outcome.ok) {
      failures.push(outcome.message);
    }
  }

  return { name: scenario.name, passed: failures.length === 0, failures };
}
