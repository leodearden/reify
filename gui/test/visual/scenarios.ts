// Visual regression scenario catalogue.
//
// Extracted from run.ts so the catalogue can be unit-tested headlessly via
// vitest (scenarios.test.ts) while the live pixel-diff harness stays in run.ts.
// Pattern mirrors gui/test/visual/paths.ts (pure module, no side-effects).

import * as path from "node:path";

export interface Camera {
  position: [number, number, number];
  target: [number, number, number];
  up?: [number, number, number];
  zoom?: number;
}

export interface Scenario {
  name: string;
  fixture: string;
  camera: Camera;
  /**
   * When set, the visual-regression harness selects this FEA load case via
   * the `set_fea_case` debug-MCP tool before taking the screenshot. Used by
   * the fea-multi-load scenarios added in task 3026.
   */
  feaCase?: string;
  /**
   * When set, the visual-regression harness drives the FEA deformed-shape
   * view before taking the screenshot (task 2968).
   *
   * - `deformed: false` — contour scene; no deformed overlay (FEA auto-enables
   *   on solve, so no toggle is needed).
   * - `deformed: true, warp: N` — harness clicks the show-deformed toggle then
   *   the warp-preset-N button (using the existing click_element /
   *   wait_for_selector debug tools on stable testIds from task 2963).
   *   `warp` must be one of the preset values: 1, 10, or 100.
   *
   * Baselines for feaView scenarios route to gui/test/screenshots/fea/<name>.png.
   */
  feaView?: { deformed: boolean; warp?: number };
}

// ─── Pure helpers ─────────────────────────────────────────────────────────────

/**
 * Compute the extension-less base path for a scenario's baseline screenshot.
 *
 * Routing priority (highest first):
 *  1. `feaView` present → `<screenshotsDir>/fea/<scenario.name>`
 *  2. `feaCase` present → `<screenshotsDir>/fea-multi-load/<scenario.feaCase>`
 *  3. default           → `<screenshotsDir>/<scenario.name>`
 *
 * The caller appends `.png`, `.actual.png`, or `.diff.png` as needed.
 *
 * This is a **pure** function (no side-effects, no Node.js I/O) so it can be
 * unit-tested headlessly in scenarios.test.ts.
 */
/**
 * A declarative action emitted by feaViewActions() for the live harness to
 * execute against the debug-MCP.
 *
 *  - `click`           → call click_element({ testId })
 *  - `waitForSelector` → call wait_for_selector({ testId })
 */
export interface FeaViewAction {
  kind: "click" | "waitForSelector";
  testId: string;
}

/**
 * Return the ordered sequence of debug-MCP actions needed to put the viewport
 * into the deformed-shape view described by `scenario.feaView`.
 *
 * Returns an empty array for:
 *  - scenarios with no `feaView` field (plain / feaCase scenarios)
 *  - `feaView.deformed === false` (contour: FEA auto-enables on solve, no toggle)
 *
 * For `feaView.deformed === true`:
 *  1. click `fea-mode-show-deformed-toggle` (enables the deformed overlay)
 *  2. waitForSelector `fea-mode-warp-preset-<warp>` (preset renders only when deformed)
 *  3. click `fea-mode-warp-preset-<warp>` (select the warp factor)
 *
 * The testIds are the stable handles from task 2963 (FeaModeToolbar.tsx).
 *
 * This is a **pure** function (no side-effects, no I/O) so it can be
 * unit-tested headlessly in scenarios.test.ts.
 */
export function feaViewActions(scenario: Scenario): FeaViewAction[] {
  if (scenario.feaView === undefined || !scenario.feaView.deformed) {
    return [];
  }
  const warp = scenario.feaView.warp;
  const presetTestId = `fea-mode-warp-preset-${warp}`;
  return [
    { kind: "click", testId: "fea-mode-show-deformed-toggle" },
    { kind: "waitForSelector", testId: presetTestId },
    { kind: "click", testId: presetTestId },
  ];
}

export function screenshotBaseFor(scenario: Scenario, screenshotsDir: string): string {
  if (scenario.feaView !== undefined) {
    return path.join(screenshotsDir, "fea", scenario.name);
  }
  if (scenario.feaCase !== undefined) {
    return path.join(screenshotsDir, "fea-multi-load", scenario.feaCase);
  }
  return path.join(screenshotsDir, scenario.name);
}

// SCENARIOS[0] is the bootstrap fixture used to start the GUI process in run.ts.
// Keep m5_geometry_flange first so the bootstrap invariant is preserved.
export const SCENARIOS: Scenario[] = [
  {
    name: "m5_geometry_flange",
    fixture: "examples/m5_geometry_flange.ri",
    camera: {
      position: [0.15, 0.1, 0.15],
      target: [0, 0, 0],
    },
  },
  {
    // 100 mm × 20 mm × 2 mm thin-walled bracket (task ι / task-3599).
    // Camera framed to show the full 100 mm length at a readable angle.
    name: "thin_walled_bracket",
    fixture: "examples/shells/thin_walled_bracket.ri",
    camera: {
      position: [0.25, 0.15, 0.15],
      target: [0.05, 0.01, 0.001],
    },
  },
  // ── Task 3026: multi-load-case FEA case-picker (one entry per load case) ──
  //
  // All three entries point at the same self-contained fixture and share a
  // camera framed around the 0.1 m × 0.05 m × 0.002 m box body.  The
  // `feaCase` field tells run.ts to select the named load case via the
  // `set_fea_case` debug-MCP tool before taking the screenshot.
  // Baselines land in gui/test/screenshots/fea-multi-load/<feaCase>.png.
  {
    name: "fea_multi_load_operating",
    fixture: "examples/fea_multi_case_bracket.ri",
    camera: {
      position: [0.25, 0.15, 0.15],
      target: [0.05, 0.025, 0.001],
    },
    feaCase: "operating",
  },
  {
    name: "fea_multi_load_overload",
    fixture: "examples/fea_multi_case_bracket.ri",
    camera: {
      position: [0.25, 0.15, 0.15],
      target: [0.05, 0.025, 0.001],
    },
    feaCase: "overload",
  },
  {
    name: "fea_multi_load_transport",
    fixture: "examples/fea_multi_case_bracket.ri",
    camera: {
      position: [0.25, 0.15, 0.15],
      target: [0.05, 0.025, 0.001],
    },
    feaCase: "transport",
  },
  // ── Task 2968: cantilever contour + deformed-shape scenes ────────────────
  //
  // All three entries share the same self-contained fixture and camera framed
  // to show the full 1 m × 0.1 m × 0.1 m beam at a readable angle.
  // Baselines land in gui/test/screenshots/fea/<name>.png.
  //
  // (1) Undeformed von-Mises contour — FEA auto-enables on solve (no toggle).
  {
    name: "cantilever_contour",
    fixture: "gui/test/fixtures/fea/cantilever_tip_load.ri",
    camera: {
      position: [0.8, 0.4, 0.6],
      target: [0.5, 0.0, 0.0],
    },
    feaView: { deformed: false },
  },
  // (2) Deformed shape — warp 1× (true scale, small displacements visible).
  // harness clicks show-deformed toggle then warp-preset-1 button.
  {
    name: "cantilever_deformed_warp1",
    fixture: "gui/test/fixtures/fea/cantilever_tip_load.ri",
    camera: {
      position: [0.8, 0.4, 0.6],
      target: [0.5, 0.0, 0.0],
    },
    feaView: { deformed: true, warp: 1 },
  },
  // (3) Deformed shape — warp 100× (amplified, exaggerated deflection visible).
  // harness clicks show-deformed toggle then warp-preset-100 button.
  {
    name: "cantilever_deformed_warp100",
    fixture: "gui/test/fixtures/fea/cantilever_tip_load.ri",
    camera: {
      position: [0.8, 0.4, 0.6],
      target: [0.5, 0.0, 0.0],
    },
    feaView: { deformed: true, warp: 100 },
  },
];
