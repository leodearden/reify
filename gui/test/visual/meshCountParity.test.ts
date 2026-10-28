/**
 * CI-gated backstop for the cross-layer mesh-count parity invariant (task 5367).
 *
 * The live e2e smoke (gui/test/visual/smoke_mesh_count_parity_e2e.mjs) needs a
 * real webview + OCCT and therefore can never run in CI. This suite pins the
 * part of it that can actually regress — the decision function that turns four
 * debug-MCP payloads into a pass/fail verdict — as pure data, with no RPC and
 * no GUI.
 *
 * Invariant under test (follow-up from tasks 5348 and 6498):
 *   viewport_state.meshCount === count(mesh_stats.meshes,   default_visible === true)
 *                            === count(engine_state.meshes, default_visible === true)
 * ...but ONLY while demand is selective (demand_dispatch.full_scope === false).
 * Under full scope build_gui_state and build_gui_state_full_scene agree by
 * construction, so the equality is trivially true and proves nothing. The
 * default_visible === false counts are floor-gated, never compared: the viewport
 * cannot see hidden bodies.
 *
 * ASSERT ON RECORDS, NOT PROSE. Every gate returns a {gate, tool, field,
 * observed, expected} record, so the cases below check the observation itself
 * (`f.gate`, `f.observed`) rather than substring-matching an English sentence —
 * where `toContain("1")` would be satisfied by nearly any message and a
 * tool-name filter silently depends on no other gate mentioning that tool. The
 * rendered wording is pinned once, in the formatFailures block at the end,
 * which is where the live run's self-diagnosis property actually lives.
 */
import { describe, it, expect } from "vitest";
import {
  checkMeshCountParity,
  extractMeshCountInputs,
  formatFailures,
  MESH_COUNT_PARITY_MIN_RENDERED_BODIES,
} from "./meshCountParity.mjs";

const RENDERED_FIELD = "meshes[default_visible=true].length";
const HIDDEN_FIELD = "meshes[default_visible=false].length";

/** A three-way-consistent, non-vacuous, aux-free baseline the cases below perturb. */
const OK_INPUT = {
  viewportMeshCount: 7,
  meshStatsRendered: 7,
  meshStatsHidden: 0,
  engineStateRendered: 7,
  engineStateHidden: 0,
  fullScope: false,
};

type Failure = {
  gate: string;
  tool: string;
  field?: string;
  observed: unknown;
  expected?: unknown;
};

/** Every failure record for one tool. */
const forTool = (failures: Failure[], tool: string) => failures.filter((f) => f.tool === tool);

describe("checkMeshCountParity — (a) happy path", () => {
  it("passes when all three reads agree under selective demand", () => {
    expect(checkMeshCountParity(OK_INPUT)).toEqual({ ok: true, failures: [] });
  });

  it("passes the printer dogfood shape: aux bodies in the debug reads, not in the viewport", () => {
    // #6498: printer.ri's debug reads held 113 meshes while the viewport drew
    // 103. The 10 extra are hidden-by-default aux realizations, so the old
    // total-length comparison failed a healthy scene; the rendered partition
    // does not.
    expect(
      checkMeshCountParity({
        viewportMeshCount: 103,
        meshStatsRendered: 103,
        meshStatsHidden: 10,
        engineStateRendered: 103,
        engineStateHidden: 10,
        fullScope: false,
      }),
    ).toEqual({ ok: true, failures: [] });
  });
});

describe("checkMeshCountParity — (b) the 5348 regression", () => {
  // Dogfood discriminator from the parent task: the viewport rendered the full
  // scene (50 meshes) while both debug reads returned only the demanded subset
  // (17). This is the exact shape the smoke exists to catch.
  const REGRESSION = {
    viewportMeshCount: 50,
    meshStatsRendered: 17,
    meshStatsHidden: 0,
    engineStateRendered: 17,
    engineStateHidden: 0,
    fullScope: false,
  };

  it("fails", () => {
    expect(checkMeshCountParity(REGRESSION).ok).toBe(false);
  });

  it("reports BOTH drifting reads as parity failures, with the observed numbers", () => {
    // The whole verdict as data — nothing here can be satisfied by an unrelated
    // message that happens to contain "50".
    const { failures } = checkMeshCountParity(REGRESSION) as { failures: Failure[] };
    expect(failures).toEqual([
      { gate: "parity", tool: "mesh_stats", field: RENDERED_FIELD, observed: 17, expected: 50 },
      { gate: "parity", tool: "engine_state", field: RENDERED_FIELD, observed: 17, expected: 50 },
    ]);
  });
});

describe("checkMeshCountParity — (b2) a default_visible regression that reports aux as visible", () => {
  it("fails parity on both reads with the inflated rendered count", () => {
    const { failures } = checkMeshCountParity({
      viewportMeshCount: 103,
      meshStatsRendered: 113,
      meshStatsHidden: 0,
      engineStateRendered: 113,
      engineStateHidden: 0,
      fullScope: false,
    }) as { failures: Failure[] };
    expect(failures).toEqual([
      { gate: "parity", tool: "mesh_stats", field: RENDERED_FIELD, observed: 113, expected: 103 },
      { gate: "parity", tool: "engine_state", field: RENDERED_FIELD, observed: 113, expected: 103 },
    ]);
  });
});

describe("checkMeshCountParity — (c) one-sided drift is localised", () => {
  it("names engine_state only when engine_state drifts", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, engineStateRendered: 3 }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.tool)).toEqual(["engine_state"]);
  });

  it("names mesh_stats only when mesh_stats drifts", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, meshStatsRendered: 3 }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.tool)).toEqual(["mesh_stats"]);
  });

  it("carries the drifting count AND the reference it was compared against", () => {
    const { failures } = checkMeshCountParity({ ...OK_INPUT, engineStateRendered: 3 }) as {
      failures: Failure[];
    };
    expect(failures[0]!.observed).toBe(3);
    expect(failures[0]!.expected).toBe(7);
  });

  it("never compares a hidden count against the viewport", () => {
    // The viewport draws only 'show' meshes, so hidden bodies have no rendered
    // reference; a hidden count that differs between the reads is not parity.
    expect(
      checkMeshCountParity({ ...OK_INPUT, meshStatsHidden: 5, engineStateHidden: 2 }),
    ).toEqual({ ok: true, failures: [] });
  });
});

describe("checkMeshCountParity — (d) vacuity gate", () => {
  it("rejects a three-way-equal run taken under full scope", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, fullScope: true }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "vacuity",
        tool: "demand_dispatch",
        field: "full_scope",
        observed: true,
        expected: false,
      },
    ]);
  });

  it("rejects a missing full_scope reading rather than assuming selectivity", () => {
    // Same gate, but `observed` keeps "scope really was full" apart from "the
    // scope was never read" — a distinction a single prose message loses.
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, fullScope: undefined }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.gate)).toEqual(["vacuity"]);
    expect(failures[0]!.observed).toBeUndefined();
  });

  it("rejects a non-boolean full_scope reading", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, fullScope: "false" as never }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.gate)).toEqual(["vacuity"]);
    expect(failures[0]!.observed).toBe("false");
  });
});

describe("checkMeshCountParity — (e) rendered degenerate gate", () => {
  it("rejects 0 === 0 === 0 (the model failed to load)", () => {
    const { ok, failures } = checkMeshCountParity({
      ...OK_INPUT,
      viewportMeshCount: 0,
      meshStatsRendered: 0,
      engineStateRendered: 0,
    }) as { ok: boolean; failures: Failure[] };
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "degenerate",
        tool: "viewport_state",
        field: "meshCount",
        observed: 0,
        expected: MESH_COUNT_PARITY_MIN_RENDERED_BODIES,
      },
    ]);
  });

  it("rejects a nonzero count that is still below the floor", () => {
    const { ok, failures } = checkMeshCountParity({
      ...OK_INPUT,
      viewportMeshCount: 1,
      meshStatsRendered: 1,
      engineStateRendered: 1,
    }) as { ok: boolean; failures: Failure[] };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.gate)).toEqual(["degenerate"]);
    expect(failures[0]!.observed).toBe(1);
    expect(failures[0]!.expected).toBe(MESH_COUNT_PARITY_MIN_RENDERED_BODIES);
  });

  it("accepts the same run when the floor is lowered via minRenderedBodies", () => {
    expect(
      checkMeshCountParity({
        ...OK_INPUT,
        viewportMeshCount: 1,
        meshStatsRendered: 1,
        engineStateRendered: 1,
        minRenderedBodies: 1,
      }),
    ).toEqual({ ok: true, failures: [] });
  });

  it("honours a RAISED floor — the case a live driver uses to pin its fixture", () => {
    // smoke_mesh_count_parity_e2e.mjs raises the floor to each fixture's known
    // rendered body count, because the generic floor of 2 only catches a TOTAL
    // load failure: a partial load realizing 2-6 of large_assembly.ri's 7 bodies
    // clears it and, if all three layers agree on the truncated set, reports a
    // green run.
    const partialLoad = {
      ...OK_INPUT,
      viewportMeshCount: 4,
      meshStatsRendered: 4,
      engineStateRendered: 4,
    };
    expect(checkMeshCountParity(partialLoad).ok).toBe(true);
    const raised = checkMeshCountParity({ ...partialLoad, minRenderedBodies: 7 }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(raised.ok).toBe(false);
    expect(raised.failures.map((f) => f.gate)).toEqual(["degenerate"]);
    expect(raised.failures[0]!.expected).toBe(7);
  });
});

describe("checkMeshCountParity — (e2) hidden-body floor", () => {
  // The aux fixture is non-vacuous only if its hidden bodies actually reach the
  // debug reads: a regression that dropped them (5348-class, on the hidden half)
  // would otherwise pass as a consistent three-rendered-body scene.
  const AUX_OK = {
    viewportMeshCount: 3,
    meshStatsRendered: 3,
    meshStatsHidden: 4,
    engineStateRendered: 3,
    engineStateHidden: 4,
    fullScope: false,
    minRenderedBodies: 3,
    minHiddenBodies: 4,
  };

  it("passes when every read carries at least the floor of hidden bodies", () => {
    expect(checkMeshCountParity(AUX_OK)).toEqual({ ok: true, failures: [] });
    expect(
      checkMeshCountParity({ ...AUX_OK, meshStatsHidden: 6, engineStateHidden: 5 }),
    ).toEqual({ ok: true, failures: [] });
  });

  it("rejects each read whose hidden bodies vanished, once per read", () => {
    const { failures } = checkMeshCountParity({
      ...AUX_OK,
      meshStatsHidden: 0,
      engineStateHidden: 0,
    }) as { failures: Failure[] };
    expect(failures).toEqual([
      { gate: "degenerate", tool: "mesh_stats", field: HIDDEN_FIELD, observed: 0, expected: 4 },
      { gate: "degenerate", tool: "engine_state", field: HIDDEN_FIELD, observed: 0, expected: 4 },
    ]);
  });

  it("localises a hidden-floor miss to the read that missed it", () => {
    const { failures } = checkMeshCountParity({ ...AUX_OK, engineStateHidden: 2 }) as {
      failures: Failure[];
    };
    expect(failures).toEqual([
      { gate: "degenerate", tool: "engine_state", field: HIDDEN_FIELD, observed: 2, expected: 4 },
    ]);
  });

  it("defaults to a floor of 0 when minHiddenBodies is omitted, so an aux-free run passes", () => {
    const { minHiddenBodies: _omitted, ...withoutHiddenFloor } = AUX_OK;
    expect(
      checkMeshCountParity({ ...withoutHiddenFloor, meshStatsHidden: 0, engineStateHidden: 0 }),
    ).toEqual({ ok: true, failures: [] });
    expect(
      checkMeshCountParity({
        ...AUX_OK,
        minHiddenBodies: undefined,
        meshStatsHidden: 0,
        engineStateHidden: 0,
      }),
    ).toEqual({ ok: true, failures: [] });
  });

  it("falls back to 1, never 0, when a PRESENT minHiddenBodies is not a usable count", () => {
    // `0 < null` is false, so passing the floor through unchecked — or treating
    // it as omitted — would silently disable the gate the caller asked for.
    for (const badFloor of [null, NaN, "4", -1, 1.5]) {
      const { failures } = checkMeshCountParity({
        ...AUX_OK,
        meshStatsHidden: 0,
        engineStateHidden: 0,
        minHiddenBodies: badFloor as never,
      }) as { failures: Failure[] };
      expect(failures).toEqual([
        { gate: "degenerate", tool: "mesh_stats", field: HIDDEN_FIELD, observed: 0, expected: 1 },
        { gate: "degenerate", tool: "engine_state", field: HIDDEN_FIELD, observed: 0, expected: 1 },
      ]);
    }
  });

  it("does not floor-gate a hidden count it already rejected as malformed", () => {
    const { failures } = checkMeshCountParity({ ...AUX_OK, meshStatsHidden: undefined }) as {
      failures: Failure[];
    };
    expect(forTool(failures, "mesh_stats")).toEqual([
      {
        gate: "shape",
        tool: "mesh_stats",
        field: HIDDEN_FIELD,
        observed: undefined,
        expected: "non-negative-integer",
      },
    ]);
  });
});

describe("checkMeshCountParity — (f) gates are independent", () => {
  it("reports the vacuity failure AND the parity failure together", () => {
    const { ok, failures } = checkMeshCountParity({
      ...OK_INPUT,
      meshStatsRendered: 3,
      fullScope: true,
    }) as { ok: boolean; failures: Failure[] };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.gate)).toEqual(["vacuity", "parity"]);
    expect(forTool(failures, "mesh_stats").map((f) => f.observed)).toEqual([3]);
  });

  it("reports the degenerate failure AND both parity failures together", () => {
    const { failures } = checkMeshCountParity({
      ...OK_INPUT,
      viewportMeshCount: 1,
      meshStatsRendered: 4,
      engineStateRendered: 9,
    }) as { failures: Failure[] };
    expect(failures.map((f) => f.gate)).toEqual(["degenerate", "parity", "parity"]);
    expect(failures.map((f) => f.tool)).toEqual(["viewport_state", "mesh_stats", "engine_state"]);
    expect(failures.map((f) => f.observed)).toEqual([1, 4, 9]);
  });

  it("evaluates every gate, in the order vacuity, shape, degenerate, parity", () => {
    const { failures } = checkMeshCountParity({
      viewportMeshCount: 1,
      meshStatsRendered: 4,
      meshStatsHidden: "x" as never,
      engineStateRendered: 9,
      engineStateHidden: 0,
      fullScope: true,
      minHiddenBodies: 2,
    }) as { failures: Failure[] };
    expect(failures.map((f) => `${f.gate}:${f.tool}:${f.field}`)).toEqual([
      "vacuity:demand_dispatch:full_scope",
      `shape:mesh_stats:${HIDDEN_FIELD}`,
      "degenerate:viewport_state:meshCount",
      `degenerate:engine_state:${HIDDEN_FIELD}`,
      `parity:mesh_stats:${RENDERED_FIELD}`,
      `parity:engine_state:${RENDERED_FIELD}`,
    ]);
  });
});

describe("checkMeshCountParity — (g) malformed counts fail loudly", () => {
  // A missing field must surface as a named failure, never silently compare as
  // NaN (which would make `!==` true) or coerce to 0 (which would make an
  // all-missing run look degenerate-but-consistent).
  const CASES: Array<[string, unknown]> = [
    ["undefined", undefined],
    ["null", null],
    ["NaN", NaN],
    ["Infinity", Infinity],
    ["a non-integer", 2.5],
    ["a negative", -1],
    ["a numeric string", "7"],
  ];

  const FIELDS: Array<[string, string, string]> = [
    ["viewportMeshCount", "viewport_state", "meshCount"],
    ["meshStatsRendered", "mesh_stats", RENDERED_FIELD],
    ["meshStatsHidden", "mesh_stats", HIDDEN_FIELD],
    ["engineStateRendered", "engine_state", RENDERED_FIELD],
    ["engineStateHidden", "engine_state", HIDDEN_FIELD],
  ];

  for (const [label, bad] of CASES) {
    for (const [input, tool, field] of FIELDS) {
      it(`rejects ${label} ${tool}.${field} as a shape problem`, () => {
        const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, [input]: bad }) as {
          ok: boolean;
          failures: Failure[];
        };
        expect(ok).toBe(false);
        expect(forTool(failures, tool)).toEqual([
          { gate: "shape", tool, field, observed: bad, expected: "non-negative-integer" },
        ]);
      });
    }
  }

  it("does not emit a parity failure for a count it already rejected as malformed", () => {
    // Exactly one engine_state failure — the shape one. Comparing an unusable
    // value against the reference would just add noise, and a `parity` gate on
    // an unread field would claim the invariant was tested when it was not.
    const { failures } = checkMeshCountParity({
      ...OK_INPUT,
      engineStateRendered: undefined,
    }) as { failures: Failure[] };
    expect(forTool(failures, "engine_state").map((f) => f.gate)).toEqual(["shape"]);
  });

  it("suppresses BOTH parity comparisons when the reference itself is unusable", () => {
    // A malformed viewport count is the reference for both comparisons; drifting
    // reads must not be blamed against a value that was never read.
    const { failures } = checkMeshCountParity({
      ...OK_INPUT,
      viewportMeshCount: undefined,
      meshStatsRendered: 3,
    }) as { failures: Failure[] };
    expect(failures.map((f) => f.gate)).toEqual(["shape"]);
    expect(failures.map((f) => f.tool)).toEqual(["viewport_state"]);
  });
});

describe("(h) total defensiveness — a bad ARGUMENT is a reading, not a crash", () => {
  // Both functions document "never throws". A `= {}` parameter default fires
  // only on `undefined`, so `null` — precisely what a caller holds after a
  // failed read — would otherwise throw a TypeError out of the very functions
  // whose job is to REPORT unusable readings, turning a diagnosable outage into
  // an unhandled crash with no named tool.
  const BAD_ARGS: Array<[string, unknown]> = [
    ["undefined", undefined],
    ["null", null],
    ["a number", 42],
    ["a string", "viewport_state"],
  ];

  for (const [label, bad] of BAD_ARGS) {
    it(`checkMeshCountParity(${label}) reports failures instead of throwing`, () => {
      expect(() => checkMeshCountParity(bad as never)).not.toThrow();
      const { ok, failures } = checkMeshCountParity(bad as never) as {
        ok: boolean;
        failures: Failure[];
      };
      expect(ok).toBe(false);
      // Every field is unreadable, so every gate that can fire, fires — and each
      // says so as a shape/vacuity problem, never as parity.
      expect(failures.map((f) => `${f.gate}:${f.tool}`)).toEqual([
        "vacuity:demand_dispatch",
        "shape:viewport_state",
        "shape:mesh_stats",
        "shape:mesh_stats",
        "shape:engine_state",
        "shape:engine_state",
      ]);
    });

    it(`extractMeshCountInputs(${label}) names all four tools instead of throwing`, () => {
      expect(() => extractMeshCountInputs(bad as never)).not.toThrow();
      const { inputs, failures } = extractMeshCountInputs(bad as never) as {
        inputs: Record<string, unknown>;
        failures: Failure[];
      };
      expect(failures.map((f) => f.tool).sort()).toEqual([
        "demand_dispatch",
        "engine_state",
        "mesh_stats",
        "viewport_state",
      ]);
      // And nothing downstream could mistake the result for a real reading.
      expect(inputs).toEqual({
        viewportMeshCount: undefined,
        meshStatsRendered: undefined,
        meshStatsHidden: undefined,
        engineStateRendered: undefined,
        engineStateHidden: undefined,
        fullScope: undefined,
      });
    });
  }

  it("falls back to the default floor when minRenderedBodies is not a usable count", () => {
    // `1 < null` is false, so passing the floor through unchecked would SILENTLY
    // DISABLE the degeneracy gate — the exact failure that gate exists to catch,
    // reintroduced through its own parameter.
    for (const badFloor of [null, undefined, NaN, "2", -1, 1.5]) {
      const { ok, failures } = checkMeshCountParity({
        ...OK_INPUT,
        viewportMeshCount: 1,
        meshStatsRendered: 1,
        engineStateRendered: 1,
        minRenderedBodies: badFloor as never,
      }) as { ok: boolean; failures: Failure[] };
      expect(ok).toBe(false);
      expect(failures.map((f) => f.gate)).toEqual(["degenerate"]);
      expect(failures[0]!.expected).toBe(MESH_COUNT_PARITY_MIN_RENDERED_BODIES);
    }
  });
});

// ─── extractMeshCountInputs ──────────────────────────────────────────────────
//
// The layer where drift against the real Rust / frontend payload shapes would
// bite. Every literal below mirrors a shape verified on base:
//   viewport_state  → { meshCount, meshInfo: [{ entityPath, ... }], ... }                bridge.ts
//   mesh_stats      → { meshes: [{ entity_path, vertex_count, default_visible, ... }] }  commands.rs
//   engine_state    → { meshes: [{ entity_path, default_visible, ... }], values, ... }   commands.rs
//   demand_dispatch → { dispatch_by_realization, eval_set, full_scope }                  commands.rs
// Note the casing seam: the frontend speaks camelCase, Rust speaks snake_case.

/** Realistic 3-body payload set, every body rendered, selective demand active. */
function livePayloads() {
  return {
    viewportState: {
      camera: { position: { x: 0, y: 0, z: 10 }, fov: 50, near: 0.1, far: 1000 },
      meshCount: 3,
      meshInfo: [
        { entityPath: "BoxPart#realization[0]", vertexCount: 24, faceCount: 12, material: null },
        { entityPath: "TubePin#realization[0]", vertexCount: 96, faceCount: 64, material: null },
        { entityPath: "BasePlate#realization[0]", vertexCount: 24, faceCount: 12, material: null },
      ],
      selectedEntity: null,
      selectedEntities: [],
      sceneBounds: null,
    },
    meshStats: {
      meshes: [
        {
          entity_path: "BoxPart#realization[0]",
          vertex_count: 24,
          face_count: 12,
          element_kind_count: { "1": 12 },
          bounding_box: { min: [0, 0, 0], max: [1, 1, 1] },
          default_visible: true as unknown,
        },
        {
          entity_path: "TubePin#realization[0]",
          vertex_count: 96,
          face_count: 64,
          element_kind_count: { "1": 64 },
          bounding_box: { min: [0, 0, 0], max: [2, 2, 2] },
          default_visible: true as unknown,
        },
        {
          entity_path: "BasePlate#realization[0]",
          vertex_count: 24,
          face_count: 12,
          element_kind_count: { "1": 12 },
          bounding_box: { min: [0, 0, 0], max: [5, 5, 1] },
          default_visible: true as unknown,
        },
      ],
    },
    engineState: {
      meshes: [
        {
          entity_path: "BoxPart#realization[0]",
          vertex_count: 24,
          face_count: 12,
          has_normals: true,
          default_visible: true as unknown,
        },
        {
          entity_path: "TubePin#realization[0]",
          vertex_count: 96,
          face_count: 64,
          has_normals: true,
          default_visible: true as unknown,
        },
        {
          entity_path: "BasePlate#realization[0]",
          vertex_count: 24,
          face_count: 12,
          has_normals: true,
          default_visible: true as unknown,
        },
      ],
      values: {},
      constraints: [],
      files: [],
      compile_diagnostics: [],
      tessellation_diagnostics: [],
      stale: false,
      reload_error: null,
    },
    demandDispatch: {
      dispatch_by_realization: { "BoxPart#realization[0]": 1 },
      eval_set: ["BoxPart#realization[0]", "TubePin#realization[0]", "BasePlate#realization[0]"],
      full_scope: false,
    },
  };
}

/**
 * An aux-bearing payload set shaped like 6752's flange: the viewport draws the
 * one rendered body, while both debug reads also carry two hidden-by-default
 * realizations.
 */
function auxPayloads() {
  const flangeMeshes = () => [
    { entity_path: "Flange#realization[0]", vertex_count: 24, face_count: 12, default_visible: false },
    { entity_path: "Flange#realization[1]", vertex_count: 24, face_count: 12, default_visible: false },
    { entity_path: "Flange#realization[2]", vertex_count: 96, face_count: 64, default_visible: true },
  ];
  const p = livePayloads();
  return {
    ...p,
    viewportState: {
      ...p.viewportState,
      meshCount: 1,
      meshInfo: [
        { entityPath: "Flange#realization[2]", vertexCount: 96, faceCount: 64, material: null },
      ],
    },
    meshStats: { meshes: flangeMeshes() },
    engineState: { ...p.engineState, meshes: flangeMeshes() },
  };
}

describe("extractMeshCountInputs — (a) real payload shapes", () => {
  it("flattens the four live payloads into the checker's input shape", () => {
    const { inputs, failures } = extractMeshCountInputs(livePayloads());
    expect(failures).toEqual([]);
    expect(inputs).toEqual({
      viewportMeshCount: 3,
      meshStatsRendered: 3,
      meshStatsHidden: 0,
      engineStateRendered: 3,
      engineStateHidden: 0,
      fullScope: false,
    });
  });

  it("partitions an aux-bearing read by default_visible", () => {
    const { inputs, failures } = extractMeshCountInputs(auxPayloads());
    expect(failures).toEqual([]);
    expect(inputs).toEqual({
      viewportMeshCount: 1,
      meshStatsRendered: 1,
      meshStatsHidden: 2,
      engineStateRendered: 1,
      engineStateHidden: 2,
      fullScope: false,
    });
  });

  it("feeds straight into checkMeshCountParity", () => {
    const { inputs } = extractMeshCountInputs(livePayloads());
    expect(checkMeshCountParity({ ...inputs, minRenderedBodies: 2 })).toEqual({
      ok: true,
      failures: [],
    });
    const aux = extractMeshCountInputs(auxPayloads()).inputs;
    expect(
      checkMeshCountParity({ ...aux, minRenderedBodies: 1, minHiddenBodies: 2 }),
    ).toEqual({ ok: true, failures: [] });
  });

  it("counts the default_visible partition, not the array length", () => {
    // Flip one mesh_stats entry to hidden: one count moves from rendered to
    // hidden, which is what makes the parity comparison downstream meaningful.
    const p = livePayloads();
    p.meshStats.meshes[1]!.default_visible = false;
    const { inputs } = extractMeshCountInputs(p);
    expect(inputs.meshStatsRendered).toBe(2);
    expect(inputs.meshStatsHidden).toBe(1);
    expect(inputs.viewportMeshCount).toBe(3);
    expect(inputs.engineStateRendered).toBe(3);
    expect(inputs.engineStateHidden).toBe(0);
  });

  it("selects the meshCount FIELD, never meshInfo.length", () => {
    // A field-selection pin on the extractor, not a production-drift guard:
    // bridge.ts builds `meshCount` (getSceneMeshes().size) and `meshInfo` from
    // the SAME map in the SAME call, so they cannot actually disagree live. The
    // divergence below is synthetic, and exists only to make the extractor's
    // choice of field observable — otherwise a silent swap to `meshInfo.length`
    // would pass every other case in this file unchanged.
    const p = livePayloads();
    p.viewportState.meshInfo = p.viewportState.meshInfo.slice(0, 1);
    expect(extractMeshCountInputs(p).inputs.viewportMeshCount).toBe(3);
  });
});

describe("extractMeshCountInputs — (a2) default_visible must be a boolean", () => {
  // The Rust join emits `null` when no entity-tree realization node matched a
  // mesh. Counting that as rendered or as hidden would blame the wrong layer, so
  // it is a named shape problem — one record per read, at the first offender.
  const BAD_FLAGS: Array<[string, unknown]> = [
    ["null", null],
    ["a string", "true"],
    ["a number", 1],
  ];

  for (const [label, bad] of BAD_FLAGS) {
    it(`rejects ${label} default_visible, leaving only that read's counts undefined`, () => {
      const p = livePayloads();
      p.meshStats.meshes[1]!.default_visible = bad;
      const { inputs, failures } = extractMeshCountInputs(p) as {
        inputs: Record<string, unknown>;
        failures: Failure[];
      };
      expect(failures).toEqual([
        {
          gate: "shape",
          tool: "mesh_stats",
          field: "meshes[1].default_visible",
          observed: bad,
          expected: "boolean",
        },
      ]);
      expect(inputs.meshStatsRendered).toBeUndefined();
      expect(inputs.meshStatsHidden).toBeUndefined();
      expect(inputs.engineStateRendered).toBe(3);
      expect(inputs.engineStateHidden).toBe(0);
    });
  }

  it("rejects a missing default_visible as a shape problem with observed undefined", () => {
    const p = livePayloads();
    delete (p.engineState.meshes[0] as Record<string, unknown>).default_visible;
    const { inputs, failures } = extractMeshCountInputs(p) as {
      inputs: Record<string, unknown>;
      failures: Failure[];
    };
    expect(failures).toEqual([
      {
        gate: "shape",
        tool: "engine_state",
        field: "meshes[0].default_visible",
        observed: undefined,
        expected: "boolean",
      },
    ]);
    expect(inputs.engineStateRendered).toBeUndefined();
    expect(inputs.engineStateHidden).toBeUndefined();
    expect(inputs.meshStatsRendered).toBe(3);
  });

  it("names only the FIRST offending entry, once per read, when the flag is gone wholesale", () => {
    const p = livePayloads();
    for (const mesh of [...p.meshStats.meshes, ...p.engineState.meshes]) {
      mesh.default_visible = null;
    }
    p.meshStats.meshes[0]!.default_visible = true;
    const { failures } = extractMeshCountInputs(p) as { failures: Failure[] };
    expect(failures.map((f) => `${f.tool}.${f.field}`)).toEqual([
      "mesh_stats.meshes[1].default_visible",
      "engine_state.meshes[0].default_visible",
    ]);
  });

  it("does not throw on a mesh entry that is not an object", () => {
    const p = livePayloads();
    (p.meshStats.meshes as unknown[])[2] = null;
    expect(() => extractMeshCountInputs(p)).not.toThrow();
    expect((extractMeshCountInputs(p).failures as Failure[]).map((f) => f.field)).toEqual([
      "meshes[2].default_visible",
    ]);
  });
});

describe("extractMeshCountInputs — (b) in-band tool errors", () => {
  // docs/debug-mcp-contract.md §2a: debug handlers report failure as
  // Ok({error: "..."}) with no MCP isError flag. A driver's rpc() hands that
  // payload back verbatim — normalizeRpcEnvelope shapes both dialects into it
  // but deliberately does not judge them — so without this check the counts
  // would silently come back `undefined`.
  const TOOLS = [
    ["viewportState", "viewport_state", ["viewportMeshCount"]],
    ["meshStats", "mesh_stats", ["meshStatsRendered", "meshStatsHidden"]],
    ["engineState", "engine_state", ["engineStateRendered", "engineStateHidden"]],
    ["demandDispatch", "demand_dispatch", ["fullScope"]],
  ] as const;

  for (const [key, toolName, fields] of TOOLS) {
    it(`reports an errored ${toolName} as an OUTAGE, carrying the handler's message`, () => {
      // gate 'outage', not 'shape': the distinction is what tells a caller the
      // invariant was never tested rather than tested-and-violated.
      const p = { ...livePayloads(), [key]: { error: "no active session" } };
      const { failures } = extractMeshCountInputs(p as never) as { failures: Failure[] };
      expect(failures).toEqual([
        { gate: "outage", tool: toolName, observed: "no active session" },
      ]);
    });

    it(`leaves ${fields.join("/")} undefined for an errored ${toolName}, never a usable-looking value`, () => {
      const p = { ...livePayloads(), [key]: { error: "boom" } };
      const { inputs, failures } = extractMeshCountInputs(p as never) as {
        inputs: Record<string, unknown>;
        failures: Failure[];
      };
      // Both halves matter, and neither implies the other: the field must not
      // carry anything downstream code could mistake for a real reading, AND
      // the outage must be named — an undefined field with no named failure
      // would surface as a mere shape problem.
      for (const field of fields) {
        expect(inputs[field]).toBeUndefined();
      }
      expect(forTool(failures, toolName).map((f) => f.gate)).toEqual(["outage"]);
    });
  }

  it("treats a non-string error field as a normal payload, not an in-band error", () => {
    // §2a's discriminator is specifically a top-level STRING `error`; a handler
    // is free to use other shapes, and misfiring here would mask real data.
    const p = livePayloads();
    (p.engineState as Record<string, unknown>).error = null;
    const { inputs, failures } = extractMeshCountInputs(p);
    expect(failures).toEqual([]);
    expect(inputs.engineStateRendered).toBe(3);
  });
});

describe("extractMeshCountInputs — (c) missing / malformed payloads", () => {
  it("names each tool whose payload is null or undefined, as a shape problem", () => {
    const { failures } = extractMeshCountInputs({
      viewportState: null,
      meshStats: undefined,
      engineState: null,
      demandDispatch: undefined,
    }) as { failures: Failure[] };
    expect(failures).toEqual([
      { gate: "shape", tool: "viewport_state", observed: null, expected: "object" },
      { gate: "shape", tool: "mesh_stats", observed: undefined, expected: "object" },
      { gate: "shape", tool: "engine_state", observed: null, expected: "object" },
      { gate: "shape", tool: "demand_dispatch", observed: undefined, expected: "object" },
    ]);
  });

  it("does not throw on a wholly empty argument", () => {
    expect(() => extractMeshCountInputs({})).not.toThrow();
    expect(extractMeshCountInputs({}).failures.length).toBeGreaterThanOrEqual(4);
  });

  it("names mesh_stats when the meshes key is missing", () => {
    const p = livePayloads();
    delete (p.meshStats as Record<string, unknown>).meshes;
    const { failures } = extractMeshCountInputs(p) as { failures: Failure[] };
    expect(failures).toEqual([
      { gate: "shape", tool: "mesh_stats", field: "meshes", observed: undefined, expected: "array" },
    ]);
  });

  it("names engine_state when meshes is present but not an array", () => {
    const p = livePayloads();
    (p.engineState as Record<string, unknown>).meshes = { "0": {} };
    const { failures } = extractMeshCountInputs(p) as { failures: Failure[] };
    expect(failures).toEqual([
      {
        gate: "shape",
        tool: "engine_state",
        field: "meshes",
        observed: { "0": {} },
        expected: "array",
      },
    ]);
  });

  it("names viewport_state when meshCount is absent or not a count", () => {
    const p = livePayloads();
    delete (p.viewportState as Record<string, unknown>).meshCount;
    expect((extractMeshCountInputs(p).failures as Failure[])[0]).toEqual({
      gate: "shape",
      tool: "viewport_state",
      field: "meshCount",
      observed: undefined,
      expected: "non-negative-integer",
    });

    const q = livePayloads();
    (q.viewportState as Record<string, unknown>).meshCount = "3";
    expect((extractMeshCountInputs(q).failures as Failure[])[0]!.observed).toBe("3");
  });

  it("names demand_dispatch when full_scope is absent or not a boolean", () => {
    const p = livePayloads();
    delete (p.demandDispatch as Record<string, unknown>).full_scope;
    expect((extractMeshCountInputs(p).failures as Failure[])[0]).toEqual({
      gate: "shape",
      tool: "demand_dispatch",
      field: "full_scope",
      observed: undefined,
      expected: "boolean",
    });

    const q = livePayloads();
    (q.demandDispatch as Record<string, unknown>).full_scope = "false";
    expect((extractMeshCountInputs(q).failures as Failure[])[0]!.observed).toBe("false");
  });

  it("does not read fullScope from the camelCase key (the Rust payload is snake_case)", () => {
    const p = livePayloads();
    delete (p.demandDispatch as Record<string, unknown>).full_scope;
    (p.demandDispatch as Record<string, unknown>).fullScope = false;
    const { inputs, failures } = extractMeshCountInputs(p) as {
      inputs: Record<string, unknown>;
      failures: Failure[];
    };
    expect(failures.map((f) => f.field)).toEqual(["full_scope"]);
    expect(inputs.fullScope).toBeUndefined();
  });
});

describe("extractMeshCountInputs — (d) full_scope: true extracts faithfully", () => {
  it("does not normalise a full-scope reading away", () => {
    const p = livePayloads();
    p.demandDispatch.full_scope = true;
    const { inputs, failures } = extractMeshCountInputs(p);
    expect(failures).toEqual([]);
    expect(inputs.fullScope).toBe(true);
  });

  it("leaves rejection of full scope to checkMeshCountParity's vacuity gate", () => {
    const p = livePayloads();
    p.demandDispatch.full_scope = true;
    const { inputs } = extractMeshCountInputs(p);
    const parity = checkMeshCountParity(inputs) as { ok: boolean; failures: Failure[] };
    expect(parity.ok).toBe(false);
    expect(parity.failures.map((f) => f.gate)).toEqual(["vacuity"]);
  });
});

describe("formatFailures — the one place a record becomes a sentence", () => {
  // The live run is read by a human, so each rendered line must still be
  // self-diagnosing: name the offending read AND the number observed. Pinned
  // HERE, once, instead of re-asserted as substring matches across every gate.
  it("renders a parity failure with both counts and the drifting tool", () => {
    const { failures } = checkMeshCountParity({
      ...OK_INPUT,
      viewportMeshCount: 50,
      meshStatsRendered: 17,
      engineStateRendered: 17,
    });
    const [first] = formatFailures(failures);
    expect(first).toContain("mesh_stats.meshes[default_visible=true].length");
    expect(first).toContain("(17)");
    expect(first).toContain("(50)");
  });

  it("renders the vacuity failure so a live run can tell WHICH gate fired", () => {
    const { failures } = checkMeshCountParity({ ...OK_INPUT, fullScope: true });
    const [msg] = formatFailures(failures);
    expect(msg).toContain("full_scope");
    expect(msg).toContain("VACUOUS");
  });

  it("renders the degenerate failure with the observed count and the floor", () => {
    const { failures } = checkMeshCountParity({ ...OK_INPUT, minRenderedBodies: 9 });
    const [msg] = formatFailures(failures);
    expect(msg).toContain("viewport_state.meshCount is 7");
    expect(msg).toContain("floor of 9");
  });

  it("renders a hidden-floor miss with the field, the observed count and the floor", () => {
    const { failures } = checkMeshCountParity({
      ...OK_INPUT,
      engineStateHidden: 0,
      minHiddenBodies: 4,
    });
    const lines = formatFailures(failures);
    expect(lines).toHaveLength(2);
    expect(lines[1]).toContain("engine_state.meshes[default_visible=false].length is 0");
    expect(lines[1]).toContain("floor of 4");
  });

  it("renders a non-boolean default_visible with the offending entry and value", () => {
    const p = livePayloads();
    p.meshStats.meshes[0]!.default_visible = null;
    const [msg] = formatFailures(extractMeshCountInputs(p).failures);
    expect(msg).toContain("mesh_stats.meshes[0].default_visible is not a boolean: null");
  });

  it("renders an outage with the handler's message and the contract reference", () => {
    const p = { ...livePayloads(), meshStats: { error: "engine poisoned" } };
    const { failures } = extractMeshCountInputs(p as never);
    const [msg] = formatFailures(failures);
    expect(msg).toContain("mesh_stats");
    expect(msg).toContain("engine poisoned");
    expect(msg).toContain("§2a");
  });

  it("distinguishes a missing field from a present-but-wrong one", () => {
    const missing = formatFailures(
      extractMeshCountInputs({ ...livePayloads(), meshStats: {} } as never).failures,
    );
    expect(missing[0]).toContain("mesh_stats.meshes is missing or not an array");

    const wrong = formatFailures(
      extractMeshCountInputs({ ...livePayloads(), meshStats: { meshes: 3 } } as never).failures,
    );
    expect(wrong[0]).toContain("mesh_stats.meshes is not an array: 3");
  });

  it("renders one line per failure, in the order the gates fired", () => {
    const { failures } = checkMeshCountParity({
      ...OK_INPUT,
      viewportMeshCount: 1,
      meshStatsRendered: 4,
      engineStateRendered: 9,
      fullScope: true,
    });
    expect(formatFailures(failures)).toHaveLength(failures.length);
    expect(formatFailures(failures)).toHaveLength(4);
  });

  it("never throws on a malformed or unrecognised record", () => {
    // A renderer that crashes takes down the diagnostic it was about to print.
    expect(() => formatFailures(null as never)).not.toThrow();
    expect(formatFailures(null as never)).toEqual([]);
    expect(() =>
      formatFailures([null, 42, { gate: "future-gate", tool: "mesh_stats", observed: 1 }] as never),
    ).not.toThrow();
    const rendered = formatFailures([
      { gate: "future-gate", tool: "mesh_stats", observed: 1 },
    ] as never);
    expect(rendered[0]).toContain("mesh_stats");
    expect(rendered[0]).toContain("future-gate");
  });
});

describe("extractMeshCountInputs — composes with checkMeshCountParity", () => {
  it("keeps read failures in their OWN list, so a caller can refuse to evaluate parity", () => {
    // The two lists stay SEPARATE, and that separation is load-bearing: a tool
    // outage leaves its fields `undefined`, over which the parity checker would
    // emit further, misleading shape failures under a headline blaming a
    // 5348-class cross-layer regression. The live driver therefore reports
    // extraction failures under their own headline and never calls
    // checkMeshCountParity at all — a failed READ is not a failed INVARIANT.
    const p = livePayloads();
    p.meshStats = { error: "engine poisoned" } as never;
    p.engineState.meshes = p.engineState.meshes.slice(0, 1);

    // `inputs` keeps its inferred type here — it is handed straight back to
    // checkMeshCountParity below, which is the composition under test.
    const { inputs, failures } = extractMeshCountInputs(p);

    // The outage is fully diagnosed WITHOUT consulting the parity checker, and
    // is labelled as one — the gate a caller branches on.
    expect(failures).toEqual([
      { gate: "outage", tool: "mesh_stats", observed: "engine poisoned" },
    ]);

    // Genuine drift in a read that DID succeed is still the parity gate's call —
    // but note what ELSE the checker emits for the errored tool: `shape`
    // failures over the `undefined`s its outage left behind. Those misleading
    // lines under a "parity violated" headline are precisely why the driver
    // short-circuits on a non-empty extraction list instead of calling this at all.
    const parity = checkMeshCountParity(inputs) as { ok: boolean; failures: Failure[] };
    expect(parity.ok).toBe(false);
    expect(parity.failures.map((f) => `${f.gate}:${f.tool}`)).toEqual([
      "shape:mesh_stats",
      "shape:mesh_stats",
      "parity:engine_state",
    ]);
  });

  it("yields a clean pass for a healthy selective-demand run", () => {
    const { inputs, failures } = extractMeshCountInputs(livePayloads());
    const parity = checkMeshCountParity(inputs);
    expect([...failures, ...parity.failures]).toEqual([]);
    expect(parity.ok).toBe(true);
  });
});
