// Visual regression scenario catalogue.
//
// Extracted from run.ts so the catalogue can be unit-tested headlessly via
// vitest (scenarios.test.ts) while the live pixel-diff harness stays in run.ts.
// Pattern mirrors gui/test/visual/paths.ts (pure module, no side-effects).

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
];
