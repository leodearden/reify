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
 */
import { describe, it, expect } from "vitest";
import {
  checkMeshCountParity,
  extractMeshCountInputs,
  isInBandError,
  MESH_COUNT_PARITY_MIN_BODIES,
} from "./meshCountParity.mjs";

/** A three-way-consistent, non-vacuous baseline the cases below perturb. */
const OK_INPUT = {
  viewportMeshCount: 7,
  meshStatsCount: 7,
  engineStateCount: 7,
  fullScope: false,
};

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

  it("names BOTH drifting reads", () => {
    const { failures } = checkMeshCountParity(REGRESSION);
    expect(failures.some((f) => f.includes("mesh_stats"))).toBe(true);
    expect(failures.some((f) => f.includes("engine_state"))).toBe(true);
  });

  it("reports the observed 50 vs 17 numbers", () => {
    const joined = checkMeshCountParity(REGRESSION).failures.join("\n");
    expect(joined).toContain("50");
    expect(joined).toContain("17");
  });
});

describe("checkMeshCountParity — (c) one-sided drift is localised", () => {
  it("names engine_state only when engine_state drifts", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, engineStateCount: 3 });
    expect(ok).toBe(false);
    expect(failures.filter((f) => f.includes("engine_state"))).toHaveLength(1);
    expect(failures.filter((f) => f.includes("mesh_stats"))).toHaveLength(0);
  });

  it("names mesh_stats only when mesh_stats drifts", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, meshStatsCount: 3 });
    expect(ok).toBe(false);
    expect(failures.filter((f) => f.includes("mesh_stats"))).toHaveLength(1);
    expect(failures.filter((f) => f.includes("engine_state"))).toHaveLength(0);
  });

  it("reports both observed numbers for the drifting read", () => {
    const { failures } = checkMeshCountParity({ ...OK_INPUT, engineStateCount: 3 });
    const msg = failures.find((f) => f.includes("engine_state"))!;
    expect(msg).toContain("3");
    expect(msg).toContain("7");
  });
});

describe("checkMeshCountParity — (d) vacuity gate", () => {
  it("rejects a three-way-equal run taken under full scope", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, fullScope: true });
    expect(ok).toBe(false);
    expect(failures).toHaveLength(1);
  });

  it("names full_scope, so a live run can tell which gate fired", () => {
    // Token, not prose: the message is free to be reworded, but it must stay
    // attributable to the vacuity gate rather than to a count mismatch.
    const [msg] = checkMeshCountParity({ ...OK_INPUT, fullScope: true }).failures;
    expect(msg).toContain("full_scope");
  });

  it("rejects a missing full_scope reading rather than assuming selectivity", () => {
    const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, fullScope: undefined });
    expect(ok).toBe(false);
    expect(failures.some((f) => f.includes("full_scope"))).toBe(true);
  });
});

describe("checkMeshCountParity — (e) degenerate gate", () => {
  it("rejects 0 === 0 === 0 (the model failed to load)", () => {
    const { ok, failures } = checkMeshCountParity({
      viewportMeshCount: 0,
      meshStatsCount: 0,
      engineStateCount: 0,
      fullScope: false,
    });
    expect(ok).toBe(false);
    expect(failures.join("\n")).toContain(String(MESH_COUNT_PARITY_MIN_BODIES));
  });

  it("rejects a nonzero count that is still below the floor", () => {
    const { ok } = checkMeshCountParity({
      viewportMeshCount: 1,
      meshStatsCount: 1,
      engineStateCount: 1,
      fullScope: false,
    });
    expect(ok).toBe(false);
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

  it("names the observed count and the floor", () => {
    const { failures } = checkMeshCountParity({
      viewportMeshCount: 1,
      meshStatsCount: 1,
      engineStateCount: 1,
      fullScope: false,
    });
    const msg = failures.find((f) => f.includes("viewport_state"))!;
    expect(msg).toContain("1");
    expect(msg).toContain("2");
  });
});

describe("checkMeshCountParity — (f) gates are independent", () => {
  it("reports the vacuity failure AND the parity failure together", () => {
    const { ok, failures } = checkMeshCountParity({
      ...OK_INPUT,
      meshStatsCount: 3,
      fullScope: true,
    });
    expect(ok).toBe(false);
    expect(failures.length).toBeGreaterThanOrEqual(2);
    expect(failures.some((f) => f.includes("full_scope"))).toBe(true);
    expect(failures.some((f) => f.includes("mesh_stats"))).toBe(true);
  });

  it("reports the degenerate failure AND both parity failures together", () => {
    const { failures } = checkMeshCountParity({
      viewportMeshCount: 1,
      meshStatsCount: 4,
      engineStateCount: 9,
      fullScope: false,
    });
    expect(failures.length).toBeGreaterThanOrEqual(3);
    expect(failures.some((f) => f.includes("viewport_state.meshCount is"))).toBe(true);
    expect(failures.some((f) => f.includes("mesh_stats"))).toBe(true);
    expect(failures.some((f) => f.includes("engine_state"))).toBe(true);
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

  for (const [label, bad] of CASES) {
    it(`rejects ${label} viewport_state.meshCount`, () => {
      const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, viewportMeshCount: bad });
      expect(ok).toBe(false);
      expect(failures.some((f) => f.includes("viewport_state.meshCount"))).toBe(true);
    });

    it(`rejects ${label} mesh_stats count`, () => {
      const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, meshStatsCount: bad });
      expect(ok).toBe(false);
      expect(failures.some((f) => f.includes("mesh_stats"))).toBe(true);
    });

    it(`rejects ${label} engine_state count`, () => {
      const { ok, failures } = checkMeshCountParity({ ...OK_INPUT, engineStateCount: bad });
      expect(ok).toBe(false);
      expect(failures.some((f) => f.includes("engine_state"))).toBe(true);
    });
  }

  it("does not emit a parity failure for a count it already rejected as malformed", () => {
    // Exactly one engine_state failure — the malformed-shape one. Comparing an
    // unusable value against the reference would just add noise.
    const { failures } = checkMeshCountParity({ ...OK_INPUT, engineStateCount: undefined });
    expect(failures.filter((f) => f.includes("engine_state"))).toHaveLength(1);
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

  it("reads viewport meshCount, not meshInfo.length, when they disagree", () => {
    // meshCount is `meshes.size` (the mesh registry); meshInfo is rebuilt by a
    // traversal. Pinning which one is authoritative catches a silent swap.
    const p = livePayloads();
    p.viewportState.meshInfo = p.viewportState.meshInfo.slice(0, 1);
    expect(extractMeshCountInputs(p).inputs.viewportMeshCount).toBe(3);
  });
});

describe("extractMeshCountInputs — (b) in-band tool errors", () => {
  // docs/debug-mcp-contract.md §2a: debug handlers report failure as
  // Ok({error: "..."}) with no MCP isError flag. The inlined rpc() helper in
  // every .mjs driver returns that payload verbatim, so without this check the
  // counts would silently come back `undefined`.
  const TOOLS = [
    ["viewportState", "viewport_state", "viewportMeshCount"],
    ["meshStats", "mesh_stats", "meshStatsCount"],
    ["engineState", "engine_state", "engineStateCount"],
    ["demandDispatch", "demand_dispatch", "fullScope"],
  ] as const;

  for (const [key, toolName, field] of TOOLS) {
    it(`names ${toolName} when it returns {error: ...}`, () => {
      const p = { ...livePayloads(), [key]: { error: "no active session" } };
      const { failures } = extractMeshCountInputs(p as never);
      expect(failures.some((f) => f.includes(toolName))).toBe(true);
      expect(failures.some((f) => f.includes("no active session"))).toBe(true);
    });

    it(`leaves ${field} undefined for an errored ${toolName}, never a usable-looking value`, () => {
      const p = { ...livePayloads(), [key]: { error: "boom" } };
      const { inputs, failures } = extractMeshCountInputs(p as never);
      // Both halves matter, and neither implies the other: the field must not
      // carry anything downstream code could mistake for a real reading, AND
      // the outage must be named — an undefined field with no named failure
      // would surface as a mere shape problem.
      expect(inputs[field]).toBeUndefined();
      expect(failures.some((f) => f.includes(toolName))).toBe(true);
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
  it("names each tool whose payload is null or undefined", () => {
    const { failures } = extractMeshCountInputs({
      viewportState: null,
      meshStats: undefined,
      engineState: null,
      demandDispatch: undefined,
    });
    for (const tool of ["viewport_state", "mesh_stats", "engine_state", "demand_dispatch"]) {
      expect(failures.some((f) => f.includes(tool))).toBe(true);
    }
  });

  it("does not throw on a wholly empty argument", () => {
    expect(() => extractMeshCountInputs({})).not.toThrow();
    expect(extractMeshCountInputs({}).failures.length).toBeGreaterThanOrEqual(4);
  });

  it("names mesh_stats when the meshes key is missing", () => {
    const p = livePayloads();
    delete (p.meshStats as Record<string, unknown>).meshes;
    const { failures } = extractMeshCountInputs(p);
    expect(failures.some((f) => f.includes("mesh_stats"))).toBe(true);
    expect(failures.some((f) => f.includes("engine_state"))).toBe(false);
  });

  it("names engine_state when meshes is present but not an array", () => {
    const p = livePayloads();
    (p.engineState as Record<string, unknown>).meshes = { "0": {} };
    const { failures } = extractMeshCountInputs(p);
    expect(failures.some((f) => f.includes("engine_state"))).toBe(true);
    expect(failures.some((f) => f.includes("mesh_stats"))).toBe(false);
  });

  it("names viewport_state when meshCount is absent or not a count", () => {
    const p = livePayloads();
    delete (p.viewportState as Record<string, unknown>).meshCount;
    expect(extractMeshCountInputs(p).failures.some((f) => f.includes("viewport_state"))).toBe(true);

    const q = livePayloads();
    (q.viewportState as Record<string, unknown>).meshCount = "3";
    expect(extractMeshCountInputs(q).failures.some((f) => f.includes("viewport_state"))).toBe(true);
  });

  it("names demand_dispatch when full_scope is absent or not a boolean", () => {
    const p = livePayloads();
    delete (p.demandDispatch as Record<string, unknown>).full_scope;
    expect(extractMeshCountInputs(p).failures.some((f) => f.includes("full_scope"))).toBe(true);

    const q = livePayloads();
    (q.demandDispatch as Record<string, unknown>).full_scope = "false";
    expect(extractMeshCountInputs(q).failures.some((f) => f.includes("full_scope"))).toBe(true);
  });

  it("does not read fullScope from the camelCase key (the Rust payload is snake_case)", () => {
    const p = livePayloads();
    delete (p.demandDispatch as Record<string, unknown>).full_scope;
    (p.demandDispatch as Record<string, unknown>).fullScope = false;
    expect(extractMeshCountInputs(p).failures.some((f) => f.includes("full_scope"))).toBe(true);
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
    const parity = checkMeshCountParity(inputs);
    expect(parity.ok).toBe(false);
    expect(parity.failures.some((f) => f.includes("full_scope"))).toBe(true);
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

describe("extractMeshCountInputs — composes with checkMeshCountParity", () => {
  it("routes extraction and parity problems through one failure list", () => {
    const p = livePayloads();
    p.meshStats = { error: "engine poisoned" } as never;
    p.engineState.meshes = p.engineState.meshes.slice(0, 1);

    const { inputs, failures } = extractMeshCountInputs(p);
    const parity = checkMeshCountParity(inputs);
    const all = [...failures, ...parity.failures];

    expect(parity.ok).toBe(false);
    expect(all.some((f) => f.includes("mesh_stats"))).toBe(true);
    expect(all.some((f) => f.includes("engine poisoned"))).toBe(true);
    expect(all.some((f) => f.includes("engine_state"))).toBe(true);
  });

  it("yields a clean pass for a healthy selective-demand run", () => {
    const { inputs, failures } = extractMeshCountInputs(livePayloads());
    const parity = checkMeshCountParity(inputs);
    expect([...failures, ...parity.failures]).toEqual([]);
    expect(parity.ok).toBe(true);
  });
});
