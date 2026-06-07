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
  /**
   * Optional setup steps executed (in order) after openFixture and BEFORE
   * the asserted tool call.  Any step returning ok:false aborts the scenario.
   */
  setup?: { tool: string; args: Record<string, unknown> }[];
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
  // not CI-gated — per H0 harness contract).  Positions verified against the committed
  // gui/test/fixtures/small_cube.ri (0-based line/col, UTF-8):
  //   line=7 col=10 → `size` identifier in `    param size: Scalar = 10mm`
  //                   (4 spaces + "param " = 10 chars, so col 10 is start of `size`)
  //   line=9 col=19 → first `size` arg in `    let body = box(size, size, size)`
  //                   ("    let body = box(" = 19 chars, so col 19 is start of first `size`)
  // If small_cube.ri is ever reformatted, re-verify with:
  //   awk 'NR==8{print substr($0,11,4)}NR==10{print substr($0,20,4)}' gui/test/fixtures/small_cube.ri
  //   (should print "size" twice; awk uses 1-based line/col hence NR=line+1, col+1)
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
  // task-4300 step-8 GREEN: I2 canvas-interaction e2e signal scenarios (live signal via
  // npm run test:e2e; NOT verify-gated — needs live reify-gui per H0 contract).
  // pick_entity_at_small_cube: centre-default ray hits the cube under the default view.
  // orbit_camera_small_cube: proves orbit_camera changes camera azimuth (threshold 0.001
  // is far below the damped single-step delta ~0.05 observed in the live GUI).
  {
    name: "pick_entity_at_small_cube",
    fixture: "small_cube",
    tool: "pick_entity_at",
    args: {},
    assertions: [
      { path: "hit", op: "equals", expected: true },
      { path: "entityPath", op: "exists" },
    ],
  },
  {
    name: "orbit_camera_small_cube",
    fixture: "small_cube",
    tool: "orbit_camera",
    args: { dazimuth: 0.5 },
    assertions: [
      { path: "ok", op: "equals", expected: true },
      { path: "azimuthDelta", op: "atLeast", expected: 0.001 },
    ],
  },
  // task-4303 F1 e2e signal scenarios (live-only via `npm run test:e2e`, NOT CI-gated
  // per PRD §4.10).  Structure validated in assertions.test.ts; live values asserted
  // only during a real reify-gui session.
  //
  // (1) load_fixture core: load all_severities.ri and assert ok===true.
  {
    name: "load_fixture_core",
    fixture: "all_severities",
    tool: "load_fixture",
    args: { name: "all_severities" },
    assertions: [{ path: "ok", op: "equals", expected: true }],
  },
  // (2) load_fixture → get_diagnostics: all_severities.ri violates thickness>5mm
  //     (thickness=1mm) → ≥1 compile diagnostic emitted via eval→compile_diagnostics.
  {
    name: "load_fixture_get_diagnostics",
    fixture: "all_severities",
    setup: [
      { tool: "load_fixture", args: { name: "all_severities" } },
      { tool: "wait_for_idle", args: {} },
    ],
    tool: "get_diagnostics",
    args: {},
    assertions: [{ path: "compileCount", op: "atLeast", expected: 1 }],
  },
  // (3) inject_diagnostics → diagnostic-row: inject 2 compile entries, open the
  //     diagnostics panel via click_element on the diagnostics-count badge, then
  //     query_selector_all diagnostic-row — asserts injected set is rendered.
  {
    name: "inject_diagnostics_diagnostic_row",
    fixture: "empty",
    setup: [
      {
        tool: "inject_diagnostics",
        args: {
          diagnostics: [
            { severity: "Error", message: "synthetic error 1" },
            { severity: "Warning", message: "synthetic warning 1" },
          ],
          source: "compile",
        },
      },
      { tool: "click_element", args: { testId: "diagnostics-count" } },
    ],
    tool: "query_selector_all",
    args: { selector: '[data-testid="diagnostic-row"]' },
    assertions: [{ path: "count", op: "atLeast", expected: 1 }],
  },
  // (4) reset_app_state → store_state baseline: load a fixture, then reset, then
  //     assert openFiles===[] and selectedEntity===null.
  {
    name: "reset_app_state_baseline",
    fixture: "small_cube",
    setup: [
      { tool: "load_fixture", args: { name: "small_cube" } },
      { tool: "wait_for_idle", args: {} },
      { tool: "reset_app_state", args: {} },
    ],
    tool: "store_state",
    args: {},
    assertions: [
      { path: "editor.openFiles", op: "equals", expected: [] },
      { path: "selection.selectedEntity", op: "equals", expected: null },
    ],
  },
  // (5) inject_diagnostics → element_screenshot of diagnostics-dialog.
  {
    name: "inject_diagnostics_element_screenshot",
    fixture: "empty",
    setup: [
      {
        tool: "inject_diagnostics",
        args: {
          diagnostics: [{ severity: "Error", message: "synthetic error for screenshot" }],
          source: "compile",
        },
      },
      { tool: "click_element", args: { testId: "diagnostics-count" } },
    ],
    tool: "element_screenshot",
    args: { testId: "diagnostics-dialog" },
    assertions: [{ path: "data", op: "exists" }],
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
 * 2. Run each setup step via deps.callTool (in order); if any returns ok:false,
 *    push a "<tool> failed" message and return early (asserted tool NOT called).
 * 3. Call deps.callTool(scenario.tool, scenario.args) — on failure push a
 *    "<tool> failed" message.
 * 4. Evaluate each assertion via evaluateAssertion, collecting failure messages.
 * 5. Return { name, passed: failures.length===0, failures }.
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

  // Run setup steps (if any) before the asserted tool.
  for (const step of scenario.setup ?? []) {
    const stepResult = await deps.callTool(step.tool, step.args);
    if (!stepResult.ok) {
      failures.push(`${step.tool} failed: ${stepResult.error}`);
      return { name: scenario.name, passed: false, failures };
    }
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
