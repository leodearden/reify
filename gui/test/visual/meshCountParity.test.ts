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
import { checkMeshCountParity, MESH_COUNT_PARITY_MIN_BODIES } from "./meshCountParity.mjs";

/** A three-way-consistent, non-vacuous baseline the cases below perturb. */
const OK_INPUT = {
  viewportMeshCount: 7,
  meshStatsCount: 7,
  engineStateCount: 7,
  fullScope: false,
};

describe("MESH_COUNT_PARITY_MIN_BODIES", () => {
  it("is the documented non-vacuity floor of 2 bodies", () => {
    expect(MESH_COUNT_PARITY_MIN_BODIES).toBe(2);
  });
});

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

  it("explains that parity under full scope is trivially true", () => {
    const [msg] = checkMeshCountParity({ ...OK_INPUT, fullScope: true }).failures;
    expect(msg).toContain("full_scope");
    expect(msg).toMatch(/trivial/i);
    expect(msg).toContain("build_gui_state_full_scene");
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
