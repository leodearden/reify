/**
 * CI-gated backstop for the cross-layer mesh-count parity invariant (task 5367).
 *
 * The live e2e smoke (gui/test/visual/smoke_mesh_count_parity_e2e.mjs) needs a
 * real webview + OCCT and therefore can never run in CI. This suite pins the
 * part of it that can actually regress — the decision function that turns four
 * debug-MCP payloads into a pass/fail verdict — as pure data, with no RPC and
 * no GUI.
 *
 * Invariant under test (follow-up from task 5348):
 *   viewport_state.meshCount === mesh_stats.meshes.length
 *                            === engine_state.meshes.length
 * ...but ONLY while demand is selective (demand_dispatch.full_scope === false).
 * Under full scope build_gui_state and build_gui_state_full_scene agree by
 * construction, so the equality is trivially true and proves nothing.
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
  isInBandError,
  normalizeRpcEnvelope,
  MESH_COUNT_PARITY_MIN_BODIES,
} from "./meshCountParity.mjs";

/** A three-way-consistent, non-vacuous baseline the cases below perturb. */
const OK_INPUT = {
  viewportMeshCount: 7,
  meshStatsCount: 7,
  engineStateCount: 7,
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
});

describe("checkMeshCountParity — (b) the 5348 regression", () => {
  // Dogfood discriminator from the parent task: the viewport rendered the full
  // scene (50 meshes) while both debug reads returned only the demanded subset
  // (17). This is the exact shape the smoke exists to catch.
  const REGRESSION = {
    viewportMeshCount: 50,
    meshStatsCount: 17,
    engineStateCount: 17,
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
      {
        gate: "parity",
        tool: "mesh_stats",
        field: "meshes.length",
        observed: 17,
        expected: 50,
      },
      {
        gate: "parity",
        tool: "engine_state",
        field: "meshes.length",
        observed: 17,
        expected: 50,
      },
    ]);
  });
});

describe("checkMeshCountParity — (c) one-sided drift is localised", () => {
  it("names engine_state only when engine_state drifts", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, engineStateCount: 3 }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.tool)).toEqual(["engine_state"]);
  });

  it("names mesh_stats only when mesh_stats drifts", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, meshStatsCount: 3 }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.tool)).toEqual(["mesh_stats"]);
  });

  it("carries the drifting count AND the reference it was compared against", () => {
    const { failures } = checkMeshCountParity({ ...OK_INPUT, engineStateCount: 3 }) as {
      failures: Failure[];
    };
    expect(failures[0]!.observed).toBe(3);
    expect(failures[0]!.expected).toBe(7);
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

describe("checkMeshCountParity — (e) degenerate gate", () => {
  it("rejects 0 === 0 === 0 (the model failed to load)", () => {
    const { ok, failures } = checkMeshCountParity({
      viewportMeshCount: 0,
      meshStatsCount: 0,
      engineStateCount: 0,
      fullScope: false,
    }) as { ok: boolean; failures: Failure[] };
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "degenerate",
        tool: "viewport_state",
        field: "meshCount",
        observed: 0,
        expected: MESH_COUNT_PARITY_MIN_BODIES,
      },
    ]);
  });

  it("rejects a nonzero count that is still below the floor", () => {
    const { ok, failures } = checkMeshCountParity({
      viewportMeshCount: 1,
      meshStatsCount: 1,
      engineStateCount: 1,
      fullScope: false,
    }) as { ok: boolean; failures: Failure[] };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.gate)).toEqual(["degenerate"]);
    expect(failures[0]!.observed).toBe(1);
    expect(failures[0]!.expected).toBe(MESH_COUNT_PARITY_MIN_BODIES);
  });

  it("accepts the same run when the floor is lowered via minBodies", () => {
    expect(
      checkMeshCountParity({
        viewportMeshCount: 1,
        meshStatsCount: 1,
        engineStateCount: 1,
        fullScope: false,
        minBodies: 1,
      }),
    ).toEqual({ ok: true, failures: [] });
  });

  it("honours a RAISED floor — the case a live driver uses to pin its fixture", () => {
    // smoke_mesh_count_parity_e2e.mjs raises the floor to large_assembly.ri's
    // known body count, because the generic floor of 2 only catches a TOTAL
    // load failure: a partial load realizing 2-6 of the 7 bodies clears it and,
    // if all three layers agree on the truncated set, reports a green run.
    const partialLoad = {
      viewportMeshCount: 4,
      meshStatsCount: 4,
      engineStateCount: 4,
      fullScope: false,
    };
    expect(checkMeshCountParity(partialLoad).ok).toBe(true);
    const raised = checkMeshCountParity({ ...partialLoad, minBodies: 7 }) as {
      ok: boolean;
      failures: Failure[];
    };
    expect(raised.ok).toBe(false);
    expect(raised.failures.map((f) => f.gate)).toEqual(["degenerate"]);
    expect(raised.failures[0]!.expected).toBe(7);
  });
});

describe("checkMeshCountParity — (f) gates are independent", () => {
  it("reports the vacuity failure AND the parity failure together", () => {
    const { ok, failures } = checkMeshCountParity({
      ...OK_INPUT,
      meshStatsCount: 3,
      fullScope: true,
    }) as { ok: boolean; failures: Failure[] };
    expect(ok).toBe(false);
    expect(failures.map((f) => f.gate)).toEqual(["vacuity", "parity"]);
    expect(forTool(failures, "mesh_stats").map((f) => f.observed)).toEqual([3]);
  });

  it("reports the degenerate failure AND both parity failures together", () => {
    const { failures } = checkMeshCountParity({
      viewportMeshCount: 1,
      meshStatsCount: 4,
      engineStateCount: 9,
      fullScope: false,
    }) as { failures: Failure[] };
    expect(failures.map((f) => f.gate)).toEqual(["degenerate", "parity", "parity"]);
    expect(failures.map((f) => f.tool)).toEqual(["viewport_state", "mesh_stats", "engine_state"]);
    expect(failures.map((f) => f.observed)).toEqual([1, 4, 9]);
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
    ["meshStatsCount", "mesh_stats", "meshes.length"],
    ["engineStateCount", "engine_state", "meshes.length"],
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
      engineStateCount: undefined,
    }) as { failures: Failure[] };
    expect(forTool(failures, "engine_state").map((f) => f.gate)).toEqual(["shape"]);
  });

  it("suppresses BOTH parity comparisons when the reference itself is unusable", () => {
    // A malformed viewport count is the reference for both comparisons; drifting
    // reads must not be blamed against a value that was never read.
    const { failures } = checkMeshCountParity({
      ...OK_INPUT,
      viewportMeshCount: undefined,
      meshStatsCount: 3,
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
      expect(failures.map((f) => f.gate)).toEqual(["vacuity", "shape", "shape", "shape"]);
      expect(failures.map((f) => f.tool)).toEqual([
        "demand_dispatch",
        "viewport_state",
        "mesh_stats",
        "engine_state",
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
        meshStatsCount: undefined,
        engineStateCount: undefined,
        fullScope: undefined,
      });
    });
  }

  it("falls back to the default floor when minBodies is not a usable count", () => {
    // `1 < null` is false, so passing the floor through unchecked would SILENTLY
    // DISABLE the degeneracy gate — the exact failure that gate exists to catch,
    // reintroduced through its own parameter.
    for (const badFloor of [null, undefined, NaN, "2", -1, 1.5]) {
      const { ok, failures } = checkMeshCountParity({
        viewportMeshCount: 1,
        meshStatsCount: 1,
        engineStateCount: 1,
        fullScope: false,
        minBodies: badFloor as never,
      }) as { ok: boolean; failures: Failure[] };
      expect(ok).toBe(false);
      expect(failures.map((f) => f.gate)).toEqual(["degenerate"]);
      expect(failures[0]!.expected).toBe(MESH_COUNT_PARITY_MIN_BODIES);
    }
  });
});

// ─── extractMeshCountInputs ──────────────────────────────────────────────────
//
// The layer where drift against the real Rust / frontend payload shapes would
// bite. Every literal below mirrors a shape verified on base:
//   viewport_state  → { meshCount, meshInfo: [{ entityPath, ... }], ... }   bridge.ts
//   mesh_stats      → { meshes: [{ entity_path, vertex_count, ... }] }      commands.rs
//   engine_state    → { meshes: [{ entity_path, ... }], values, ... }       commands.rs
//   demand_dispatch → { dispatch_by_realization, eval_set, full_scope }     commands.rs
// Note the casing seam: the frontend speaks camelCase, Rust speaks snake_case.

/** Realistic 3-body payload set, selective demand active. */
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
        },
        {
          entity_path: "TubePin#realization[0]",
          vertex_count: 96,
          face_count: 64,
          element_kind_count: { "1": 64 },
          bounding_box: { min: [0, 0, 0], max: [2, 2, 2] },
        },
        {
          entity_path: "BasePlate#realization[0]",
          vertex_count: 24,
          face_count: 12,
          element_kind_count: { "1": 12 },
          bounding_box: { min: [0, 0, 0], max: [5, 5, 1] },
        },
      ],
    },
    engineState: {
      meshes: [
        { entity_path: "BoxPart#realization[0]", vertex_count: 24, face_count: 12, has_normals: true },
        { entity_path: "TubePin#realization[0]", vertex_count: 96, face_count: 64, has_normals: true },
        { entity_path: "BasePlate#realization[0]", vertex_count: 24, face_count: 12, has_normals: true },
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

describe("extractMeshCountInputs — (a) real payload shapes", () => {
  it("flattens the four live payloads into the checker's input shape", () => {
    const { inputs, failures } = extractMeshCountInputs(livePayloads());
    expect(failures).toEqual([]);
    expect(inputs).toEqual({
      viewportMeshCount: 3,
      meshStatsCount: 3,
      engineStateCount: 3,
      fullScope: false,
    });
  });

  it("feeds straight into checkMeshCountParity", () => {
    const { inputs } = extractMeshCountInputs(livePayloads());
    expect(checkMeshCountParity({ ...inputs, minBodies: 2 })).toEqual({ ok: true, failures: [] });
  });

  it("counts mesh_stats/engine_state array LENGTH, not some sibling field", () => {
    // Drop one entry from mesh_stats only: the extractor must report 2, which
    // is what makes the parity comparison downstream meaningful.
    const p = livePayloads();
    p.meshStats.meshes = p.meshStats.meshes.slice(0, 2);
    const { inputs } = extractMeshCountInputs(p);
    expect(inputs.meshStatsCount).toBe(2);
    expect(inputs.viewportMeshCount).toBe(3);
    expect(inputs.engineStateCount).toBe(3);
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

describe("extractMeshCountInputs — (b) in-band tool errors", () => {
  // docs/debug-mcp-contract.md §2a: debug handlers report failure as
  // Ok({error: "..."}) with no MCP isError flag. A driver's rpc() hands that
  // payload back verbatim — normalizeRpcEnvelope shapes both dialects into it
  // but deliberately does not judge them — so without this check the counts
  // would silently come back `undefined`.
  const TOOLS = [
    ["viewportState", "viewport_state", "viewportMeshCount"],
    ["meshStats", "mesh_stats", "meshStatsCount"],
    ["engineState", "engine_state", "engineStateCount"],
    ["demandDispatch", "demand_dispatch", "fullScope"],
  ] as const;

  for (const [key, toolName, field] of TOOLS) {
    it(`reports an errored ${toolName} as an OUTAGE, carrying the handler's message`, () => {
      // gate 'outage', not 'shape': the distinction is what tells a caller the
      // invariant was never tested rather than tested-and-violated.
      const p = { ...livePayloads(), [key]: { error: "no active session" } };
      const { failures } = extractMeshCountInputs(p as never) as { failures: Failure[] };
      expect(failures).toEqual([
        { gate: "outage", tool: toolName, observed: "no active session" },
      ]);
    });

    it(`leaves ${field} undefined for an errored ${toolName}, never a usable-looking value`, () => {
      const p = { ...livePayloads(), [key]: { error: "boom" } };
      const { inputs, failures } = extractMeshCountInputs(p as never) as {
        inputs: Record<string, unknown>;
        failures: Failure[];
      };
      // Both halves matter, and neither implies the other: the field must not
      // carry anything downstream code could mistake for a real reading, AND
      // the outage must be named — an undefined field with no named failure
      // would surface as a mere shape problem.
      expect(inputs[field]).toBeUndefined();
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
    expect(inputs.engineStateCount).toBe(3);
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

describe("isInBandError — the tool-outage discriminator the live driver reuses", () => {
  // Exported for the smoke's selectivity precondition: a FAILED demand_dispatch
  // must be diagnosed as a tool outage, not read as `full_scope !== false` and
  // blamed on the frontend never calling sync_demand.
  it("accepts the frontend in-band shape", () => {
    expect(isInBandError({ error: "no active session" })).toBe(true);
  });

  it("accepts the Rust isError dialect once rpc() has normalised it", () => {
    // debug_server.rs answers a Rust-dispatched handler failure with
    // {content: [{type:'text', text:'Error: <msg>'}], isError: true}; the
    // driver's rpc() folds that into {error: '<text>'} so this one detector
    // covers both dialects. If it did not, the outage would arrive as a bare
    // string and be misreported as a payload-shape problem.
    expect(isInBandError({ error: "Error: engine thread died" })).toBe(true);
  });

  it("rejects a healthy payload, and non-objects", () => {
    expect(isInBandError({ full_scope: false, eval_set: [] })).toBe(false);
    expect(isInBandError(null)).toBe(false);
    expect(isInBandError(undefined)).toBe(false);
    expect(isInBandError("Error: engine thread died")).toBe(false);
    expect(isInBandError([{ error: "nested" }])).toBe(false);
  });

  it("rejects a non-string error field (§2a's discriminator is a STRING error)", () => {
    // Misfiring here would mask real data — a handler is free to use `error`
    // for something else. Matches the extractor's behaviour on the same shape.
    expect(isInBandError({ error: null, full_scope: false })).toBe(false);
    expect(isInBandError({ error: 500, full_scope: false })).toBe(false);
  });
});

describe("normalizeRpcEnvelope — the two failure dialects folded into one shape", () => {
  // The live driver's rpc() used to inline this branch, which made the ONE piece
  // of genuinely new transport logic in the smoke — folding the Rust `isError`
  // envelope into the frontend's in-band `{error}` shape so a single
  // isInBandError check covers both dialects — the only part with no CI cover.
  const textEnvelope = (value: unknown) => ({
    result: { content: [{ type: "text", text: JSON.stringify(value) }] },
  });

  it("returns the parsed payload for a healthy JSON text block", () => {
    const { transportError, payload } = normalizeRpcEnvelope(
      textEnvelope({ full_scope: false, eval_set: ["a"] }),
    );
    expect(transportError).toBeUndefined();
    expect(payload).toEqual({ full_scope: false, eval_set: ["a"] });
  });

  it("reports a top-level (transport) error separately from a payload", () => {
    // The driver throws on this branch; it must never be mistaken for a tool
    // payload, in-band error or otherwise.
    const { transportError, payload } = normalizeRpcEnvelope({
      error: { code: -32601, message: "Method not found" },
    });
    expect(typeof transportError).toBe("string");
    expect(transportError).toContain("Method not found");
    expect(payload).toBeUndefined();
  });

  it("folds the Rust isError envelope into the in-band {error} shape", () => {
    // debug_server.rs answers a Rust-dispatched handler failure with
    // isError:true plus a plain-text `Error: <msg>` block. Normalising it here
    // is what lets the driver's ONE isInBandError check cover engine_state,
    // mesh_stats and demand_dispatch as well as the frontend-mediated tools.
    const { transportError, payload } = normalizeRpcEnvelope({
      result: {
        content: [{ type: "text", text: "Error: engine thread died" }],
        isError: true,
      },
    });
    expect(transportError).toBeUndefined();
    expect(isInBandError(payload)).toBe(true);
    expect((payload as { error: string }).error).toContain("engine thread died");
  });

  it("still yields an in-band error when isError carries no text block", () => {
    // Degrading to `null` here would send the outage downstream as a shape
    // problem — the misdiagnosis this module exists to prevent.
    const { payload } = normalizeRpcEnvelope({ result: { content: [], isError: true } });
    expect(isInBandError(payload)).toBe(true);
  });

  it("yields null when there is no text block to interpret", () => {
    // Empty/absent content, or a non-text block such as the image one that the
    // screenshot tools answer with: "nothing to interpret", not a failure. No
    // caller in this module reads image data, so the block is not decoded.
    for (const result of [
      { content: [{ type: "image", data: "iVBORw0KGgo=", mimeType: "image/png" }] },
      { content: [] },
      {},
    ]) {
      const { transportError, payload } = normalizeRpcEnvelope({ result });
      expect(transportError).toBeUndefined();
      expect(payload).toBeNull();
    }
  });

  it("hands back the raw text when the block is not JSON", () => {
    const { payload } = normalizeRpcEnvelope({
      result: { content: [{ type: "text", text: "pong" }] },
    });
    expect(payload).toBe("pong");
  });

  it("never throws on a malformed or absent envelope", () => {
    for (const envelope of [undefined, null, {}, { result: null }, { result: {} }, 42]) {
      expect(() => normalizeRpcEnvelope(envelope as never)).not.toThrow();
    }
  });

  it("leaves a frontend in-band error untouched for isInBandError to catch", () => {
    // viewport_state (via query_frontend) already speaks this dialect natively;
    // normalisation must be a no-op for it rather than double-wrapping.
    const { payload } = normalizeRpcEnvelope(textEnvelope({ error: "no active session" }));
    expect(isInBandError(payload)).toBe(true);
    expect((payload as { error: string }).error).toBe("no active session");
  });
});

describe("formatFailures — the one place a record becomes a sentence", () => {
  // The live run is read by a human, so each rendered line must still be
  // self-diagnosing: name the offending read AND the number observed. Pinned
  // HERE, once, instead of re-asserted as substring matches across every gate.
  it("renders a parity failure with both counts and the drifting tool", () => {
    const { failures } = checkMeshCountParity({
      viewportMeshCount: 50,
      meshStatsCount: 17,
      engineStateCount: 17,
      fullScope: false,
    });
    const [first] = formatFailures(failures);
    expect(first).toContain("mesh_stats.meshes.length");
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
    const { failures } = checkMeshCountParity({ ...OK_INPUT, minBodies: 9 });
    const [msg] = formatFailures(failures);
    expect(msg).toContain("viewport_state.meshCount is 7");
    expect(msg).toContain("floor of 9");
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
      viewportMeshCount: 1,
      meshStatsCount: 4,
      engineStateCount: 9,
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
    // outage leaves its field `undefined`, over which the parity checker would
    // emit a second, misleading shape failure under a headline blaming a
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
    // but note what ELSE the checker emits for the errored tool: a `shape`
    // failure over the `undefined` its outage left behind. That second, misleading
    // line under a "parity violated" headline is precisely why the driver
    // short-circuits on a non-empty extraction list instead of calling this at all.
    const parity = checkMeshCountParity(inputs) as { ok: boolean; failures: Failure[] };
    expect(parity.ok).toBe(false);
    expect(parity.failures.map((f) => `${f.gate}:${f.tool}`)).toEqual([
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
