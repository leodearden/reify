/**
 * Value-assertion harness for the reify-debug e2e test suite.
 *
 * Pure module — no I/O. All integration glue lives in run.ts.
 * Mirrors the established pure-module convention (diff.ts, rpc.ts, paths.ts).
 */

import type { RpcResult } from "./rpc.js";
// The one app-runtime import this module allows itself.  orbitDistance.ts is deliberately
// dependency-free (no three, no DOM), so it loads unchanged in the bare node process that
// `tsx test/visual/run.ts` uses to drive a LIVE GUI over RPC.  Importing the real framing
// formula is what stops the camera thresholds below from being a second, drifting copy of it.
import { fittedDistanceFor } from "../../src/viewport/orbitDistance.js";

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
 *
 * COUPLING: kept in sync with `fixture_relpath()` in
 * gui/src-tauri/src/debug_server.rs. Add new fixtures to BOTH sides; an
 * "unknown fixture" runtime error from load_fixture is the only gap-detector
 * (no compile-time cross-check exists).
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
// ─── small_cube camera geometry (task 6965) ──────────────────────────────────
//
// Every camera threshold below is DERIVED from the fixture and the framing
// formula, never an observed output, so each assertion states the physics
// rather than pinning whatever the GUI happened to print.
//
//   small_cube.ri declares `param size: Length = 10mm` and `box(size,size,size)`,
//   so the framed box is a 10 mm cube — 0.01 in scene units (metres).
//   fitCameraToBox frames the circumscribing SPHERE, radius = ½·box diagonal.
const SMALL_CUBE_EDGE_M = 0.01;
const SMALL_CUBE_RADIUS = 0.5 * Math.sqrt(3) * SMALL_CUBE_EDGE_M; // ≈ 8.660e-3

/**
 * Mirror of `CAMERA_FOV_DEG` (gui/src/viewport/scene.ts).
 *
 * Mirrored rather than imported because scene.ts pulls in three and the axis-label
 * builders, which this module must stay clear of (see the import note above).
 * `assertions.test.ts` pins this against the real constant, so a retuned field of view
 * reds in the gate instead of silently invalidating every camera threshold below.
 */
export const SCENE_CAMERA_FOV_DEG = 60;

// The fitted distance comes from fitCameraToBox's own formula, at the square-pane
// reference aspect. The horizontal term binds only on a pane TALLER than wide
// (fitCamera.ts design decision 2, esc-4280) and binds UPWARDS — so this is a lower
// bound on the real fitted distance, which is what makes the atLeast/atMost pair below
// sound for any pane shape.
const SMALL_CUBE_FIT_DISTANCE = fittedDistanceFor(SMALL_CUBE_RADIUS, SCENE_CAMERA_FOV_DEG); // ≈ 1.905e-2

// zoom_camera dollies MULTIPLICATIVELY: dollyIn(scale) ⇒ distance *= scale.
const SMALL_CUBE_ZOOM_SCALE = 0.3;
const SMALL_CUBE_ZOOMED_DISTANCE = SMALL_CUBE_ZOOM_SCALE * SMALL_CUBE_FIT_DISTANCE; // 0.66·r
const SMALL_CUBE_ZOOM_DELTA = SMALL_CUBE_FIT_DISTANCE - SMALL_CUBE_ZOOMED_DISTANCE; // 1.54·r

// Headroom on the upper bound, because a tall/narrow pane fits FARTHER back (above)
// and so also lands farther back after the dolly. 8× covers aspect ratios down to
// ≈0.2 and keeps the bound (≈4.6e-2) an order of magnitude below a fixed 0.5 m floor.
const SMALL_CUBE_PANE_ASPECT_HEADROOM = 8;

// An iso-ish close-in pose at the fitted distance, used to prove a pick still
// resolves after the camera has been re-framed (#6496).
const SMALL_CUBE_FRAMED_POSE = SMALL_CUBE_FIT_DISTANCE / Math.sqrt(3); // ≈ 1.100e-2

// applied.position can differ from an unclamped request by ~1 ulp and applied.target
// is exact (docs/debug-mcp-contract.md §6 point 1), so position is bracketed with this
// tolerance — far above that drift, far below any clamp — and target uses `equals`.
const CAMERA_READBACK_TOL = 1e-9;

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
  // task-6965: the three camera commands this task repaired. Live signal via
  // `npm run test:e2e` only — NOT verify-gated, same caveat as the I2 entries above.
  // Each asserts LIVE state through the command's own response fields (set_camera's
  // `applied` and zoom_camera's `distance` are read back from the camera/controls
  // after OrbitControls has applied its constraints), never a restatement of inputs.
  //
  // zoom_camera_small_cube pins the dogfood no-op (a fixed floor saturating the dolly,
  // reported as `distanceDelta: 0`; docs/debug-mcp-contract.md §6 point 3).
  {
    name: "zoom_camera_small_cube",
    fixture: "small_cube",
    setup: [{ tool: "fit_to_view", args: {} }],
    tool: "zoom_camera",
    args: { scale: SMALL_CUBE_ZOOM_SCALE },
    assertions: [
      { path: "ok", op: "equals", expected: true },
      // The dolly actually moved the camera. Half the computed delta leaves room
      // for pane shape while staying far above the 0 the regression produced.
      { path: "distanceDelta", op: "atLeast", expected: SMALL_CUBE_ZOOM_DELTA / 2 },
      // ...and landed genuinely close to a 10 mm part, i.e. the floor now tracks
      // the model bounds instead of sitting at a fixed 0.5 m.
      {
        path: "distance",
        op: "atMost",
        expected: SMALL_CUBE_ZOOMED_DISTANCE * SMALL_CUBE_PANE_ASPECT_HEADROOM,
      },
    ],
  },
  // Pins step-12's read-back contract end to end: `applied` is the LIVE pose after
  // controls.update(), not the request echoed back. The fit_to_view setup makes the
  // orbit floor this fixture's own, not whatever an earlier scenario last framed; the
  // pose is then well inside both distance limits, so the unclamped path is exercised.
  {
    name: "set_camera_reports_live_pose",
    fixture: "small_cube",
    setup: [{ tool: "fit_to_view", args: {} }],
    tool: "set_camera",
    args: { position: [0.03, 0.03, 0.03], target: [0, 0, 0] },
    assertions: [
      { path: "ok", op: "equals", expected: true },
      // target survives update() exactly, so it pins the read-back source.
      { path: "applied.target", op: "equals", expected: [0, 0, 0] },
      // position is bracketed rather than equated — see CAMERA_READBACK_TOL.
      { path: "applied.position.0", op: "atLeast", expected: 0.03 - CAMERA_READBACK_TOL },
      { path: "applied.position.0", op: "atMost", expected: 0.03 + CAMERA_READBACK_TOL },
      { path: "applied.position.1", op: "atLeast", expected: 0.03 - CAMERA_READBACK_TOL },
      { path: "applied.position.1", op: "atMost", expected: 0.03 + CAMERA_READBACK_TOL },
      { path: "applied.position.2", op: "atLeast", expected: 0.03 - CAMERA_READBACK_TOL },
      { path: "applied.position.2", op: "atMost", expected: 0.03 + CAMERA_READBACK_TOL },
    ],
  },
  // The #6496 screenshot → set_camera → pick → identify loop as a regression
  // scenario: a pick immediately after a camera move must resolve against the pose
  // set_camera just reported, with no intervening render. pick_entity_at_small_cube
  // above deliberately exercises the DEFAULT camera and never moves it, so it cannot
  // observe the stale-matrixWorld bug at all — this is its framed counterpart, not a
  // duplicate of it. fit_to_view first, for the same fixture-owned floor as above.
  {
    name: "pick_after_set_camera_small_cube",
    fixture: "small_cube",
    setup: [
      { tool: "fit_to_view", args: {} },
      {
        tool: "set_camera",
        args: {
          position: [SMALL_CUBE_FRAMED_POSE, SMALL_CUBE_FRAMED_POSE, SMALL_CUBE_FRAMED_POSE],
          target: [0, 0, 0],
        },
      },
    ],
    tool: "pick_entity_at",
    args: {},
    assertions: [
      { path: "hit", op: "equals", expected: true },
      { path: "entityPath", op: "exists" },
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
  // task-4305 E1 I1 e2e signal; scroll drives the real CodeMirror scrollDOM.
  // scrollTop asserted via exists (not a value) because clamp depends on live viewport
  // height — exact/atLeast would be a doomed RED on tall viewports (mirrors
  // list_console_errors_shape). large_assembly (58 lines) is scrollable on typical viewports.
  {
    name: "scroll_editor_large_assembly",
    fixture: "large_assembly",
    tool: "scroll",
    args: { target: "editor", top: 1000 },
    assertions: [
      { path: "ok", op: "equals", expected: true },
      { path: "scrollTop", op: "exists" },
    ],
  },
  // task-4305 E1 C1 e2e signal; open_menu clicks [data-testid=menu-trigger-file] and
  // returns the live ctx.menuBar.openMenu() state (synchronous Solid signal update).
  {
    name: "open_menu_file",
    fixture: "small_cube",
    tool: "open_menu",
    args: { name: "file" },
    assertions: [
      { path: "ok", op: "equals", expected: true },
      { path: "open", op: "equals", expected: "file" },
    ],
  },
  // task-4305 E1 C2 e2e signal (live via npm run test:e2e); resize_panes writes
  // ctx.stores.layout (L0) and echoes layout.editorWidth — deterministic store round-trip.
  {
    name: "resize_panes_editor_width",
    fixture: "small_cube",
    tool: "resize_panes",
    args: { editorWidth: 400 },
    assertions: [
      { path: "ok", op: "equals", expected: true },
      { path: "layout.editorWidth", op: "equals", expected: 400 },
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

/**
 * Every op `evaluateAssertion` handles, and therefore every op a VALUE_SCENARIOS entry
 * may name.  This array is the SOURCE, and `AssertionOp` is derived from it — not the
 * other way round — so an op cannot exist in the type while being absent from the
 * roster.  A hand-written roster typed `readonly AssertionOp[]` would allow exactly
 * that: an array type constrains what MAY appear, never what MUST, so an omission
 * typechecks cleanly and leaves the op unguarded by every consumer that iterates it.
 *
 * Derivation rather than a `satisfies` cross-check is what makes this hold HERE:
 * `gui/tsconfig.json` includes only `src`, so nothing in this directory is typechecked
 * by the gate and a compile-time-only guard would be inert.  A derived union survives
 * that because the roster it is derived from is a runtime value the tests iterate.
 */
export const ASSERTION_OPS = ["equals", "atLeast", "atMost", "exists"] as const;

export type AssertionOp = (typeof ASSERTION_OPS)[number];

export type Assertion = {
  path: string;
  op: AssertionOp;
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
 * - 'atMost': actual must be a number <= Number(expected) — the mirror of
 *   'atLeast', for stating an UPPER bound on a value computed through floating
 *   point (an orbit distance, a delta) where 'equals' cannot tolerate the drift.
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
    case "atMost": {
      if (typeof actual === "number" && actual <= Number(a.expected)) {
        return { ok: true };
      }
      return {
        ok: false,
        message: `${a.path}: expected atMost ${String(a.expected)}, got ${JSON.stringify(actual)}`,
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

// ─── KNOWN_DEBUG_TOOL_NAMES ───────────────────────────────────────────────────

/**
 * Canonical set of debug tool names used by VALUE_SCENARIOS setup steps.
 *
 * Mirrors the handler keys registered by `buildHandlers()` in
 * gui/src/debug/bridge.ts (frontend-mediated tools) and `tool_defs()` in
 * gui/src-tauri/src/debug_server.rs (Rust-dispatched tools). The tool_defs()
 * half of that invariant (KNOWN_DEBUG_TOOL_NAMES ⊇ tool_defs()) is
 * mechanically checked by the parity test in assertions.test.ts (task-5934).
 *
 * Exported so assertions.test.ts can derive its KNOWN_TOOLS check from a single
 * source rather than maintaining an inline literal.  Every name in tool_defs()
 * MUST appear here — enforced by assertions.test.ts's parity test (b); add the
 * entry here when you add a ToolDef, whether or not any VALUE_SCENARIOS setup
 * step uses it yet. Frontend-only handler names (bridge.ts buildHandlers()) are
 * still added on demand, when a VALUE_SCENARIOS setup step first uses them.
 */
export const KNOWN_DEBUG_TOOL_NAMES: ReadonlySet<string> = new Set([
  // Rust-dispatched tools (debug_server.rs dispatch_tool, incl. its
  // dispatch_stateless_tool delegate for morph_stats/mesh_morph_stats)
  "health",
  "engine_status",
  "engine_state",
  "demand_dispatch",
  "mesh_stats",
  "morph_stats",
  "mesh_morph_stats",
  "load_fixture",
  "set_fea_case",
  "open_file",
  // AI write tools (task 5097 δ) — reify-mcp identities on the reify-debug
  // surface. reify_open_file has its own dispatch arm but shares open_file's
  // funnel (`open_path_into_engine`); the arms differ only in the result
  // envelope, since the reify-mcp identity owes its clients {success, source}.
  "reify_set_parameter",
  "reify_update_source",
  "reify_open_file",
  "reify_save_file",
  "reify_export",
  // Frontend-mediated tools (bridge.ts buildHandlers)
  "wait_for_idle",
  "wait_for",
  "wait_for_selector",
  "get_diagnostics",
  "inject_diagnostics",
  "reset_app_state",
  "click_element",
  "query_selector_all",
  "query_selector",
  "store_state",
  "element_screenshot",
  "screenshot",
  "screenshot_window",
  "type_in_editor",
  "keyboard",
  "focus_element",
  "scroll",
  "select_entity",
  "clear_selection",
  "fit_to_view",
  "set_camera",
  "set_test_mode",
  "list_console_errors",
  "resize_panes",
  "expand_tree_node",
  "collapse_tree_node",
  "hover_at",
  "completion_at",
  "definition_at",
  "pick_entity_at",
  "orbit_camera",
  "pan_camera",
  "zoom_camera",
  "viewport_state",
  "dom_query",
  "list_elements",
  "get_layout_metrics",
  "get_computed_style",
  "get_window_state",
  "ui_outline",
  "active_element",
  "apply_gui_state",
  "click_at",
  "drag",
  "editor_content",
  "focus_editor",
  "get_local_storage",
  "hover",
  "menu_state",
  "open_menu",
  "press_tab",
  "set_fea_channel",
  "set_window_size",
  "tab_order",
  "toggle_select",
] as const);
