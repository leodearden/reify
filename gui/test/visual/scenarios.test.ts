import { describe, it, expect } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { SCENARIOS } from "./scenarios.js";
import { resolveRepoRoot } from "./paths.js";

const CANTILEVER_FIXTURE = "gui/test/fixtures/fea/cantilever_tip_load.ri";

describe("SCENARIOS catalogue", () => {
  it("(a) contains exactly one entry with name === 'thin_walled_bracket'", () => {
    const entries = SCENARIOS.filter((s) => s.name === "thin_walled_bracket");
    expect(entries).toHaveLength(1);
  });

  it("(b) thin_walled_bracket fixture is 'examples/shells/thin_walled_bracket.ri'", () => {
    const entry = SCENARIOS.find((s) => s.name === "thin_walled_bracket");
    expect(entry?.fixture).toBe("examples/shells/thin_walled_bracket.ri");
  });

  it("(c) thin_walled_bracket camera has valid 3-number position and target", () => {
    const entry = SCENARIOS.find((s) => s.name === "thin_walled_bracket");
    expect(entry).toBeDefined();
    const { position, target } = entry!.camera;
    expect(position).toHaveLength(3);
    expect(target).toHaveLength(3);
    for (const v of [...position, ...target]) {
      expect(typeof v).toBe("number");
      expect(isFinite(v)).toBe(true);
    }
  });

  it("(d) all SCENARIOS names are unique and all fixtures are non-empty strings", () => {
    const names = SCENARIOS.map((s) => s.name);
    const uniqueNames = new Set(names);
    expect(uniqueNames.size).toBe(names.length);
    for (const s of SCENARIOS) {
      expect(typeof s.fixture).toBe("string");
      expect(s.fixture.length).toBeGreaterThan(0);
    }
  });

  it("(e) the pre-existing 'm5_geometry_flange' scenario is still present", () => {
    const entry = SCENARIOS.find((s) => s.name === "m5_geometry_flange");
    expect(entry).toBeDefined();
    expect(entry?.fixture).toBe("examples/m5_geometry_flange.ri");
  });

  it("(f) SCENARIOS[0] is 'm5_geometry_flange' — the bootstrap fixture for run.ts", () => {
    // run.ts spawns the GUI process using SCENARIOS[0] as the initial fixture;
    // reordering would silently change the bootstrap fixture without any other
    // test failing.  Lock the invariant here so a future reorder is caught.
    expect(SCENARIOS[0].name).toBe("m5_geometry_flange");
  });

  it("(g) every scenario's fixture file exists on disk", () => {
    // String-equality checks in (b)/(e) duplicate the source constant and give
    // no signal if the referenced .ri file is later deleted or renamed.  Resolve
    // each fixture against REPO_ROOT and assert it exists so the catalogue stays
    // in sync with the file tree.
    const repoRoot = resolveRepoRoot(import.meta.url);
    for (const s of SCENARIOS) {
      const abs = path.join(repoRoot, s.fixture);
      expect(
        fs.existsSync(abs),
        `scenario '${s.name}' fixture '${s.fixture}' not found at ${abs}`,
      ).toBe(true);
    }
  });
});

// ── Task 3026 step-19: RED — Scenario.feaCase field + fea-multi-load entries ──
//
// These tests FAIL until step-20 adds `feaCase?: string` to the Scenario
// interface and appends the three fea-multi-load entries to SCENARIOS.

describe("fea-multi-load scenarios (task 3026)", () => {
  const FEA_CASES = ["operating", "overload", "transport"] as const;
  const FEA_FIXTURE = "examples/fea_multi_case_bracket.ri";

  it("(a) SCENARIOS contains exactly three fea_multi_load_* entries", () => {
    const entries = SCENARIOS.filter((s) =>
      s.name.startsWith("fea_multi_load_"),
    );
    expect(entries).toHaveLength(3);
  });

  it("(b) each fea_multi_load entry points at fea_multi_case_bracket.ri", () => {
    for (const caseName of FEA_CASES) {
      const name = `fea_multi_load_${caseName}`;
      const entry = SCENARIOS.find((s) => s.name === name);
      expect(
        entry,
        `expected SCENARIOS to contain an entry named '${name}'`,
      ).toBeDefined();
      expect(entry!.fixture).toBe(FEA_FIXTURE);
    }
  });

  it("(c) each fea_multi_load entry has the matching feaCase value", () => {
    for (const caseName of FEA_CASES) {
      const name = `fea_multi_load_${caseName}`;
      const entry = SCENARIOS.find((s) => s.name === name);
      expect(entry).toBeDefined();
      // feaCase must be an optional string on the Scenario interface (step-20).
      // TypeScript will error here until step-20 adds `feaCase?: string`.
      expect((entry as any).feaCase).toBe(caseName);
    }
  });

  it("(d) all three fea_multi_load entries share the same camera", () => {
    const entries = FEA_CASES.map((c) =>
      SCENARIOS.find((s) => s.name === `fea_multi_load_${c}`),
    ).filter(Boolean) as (typeof SCENARIOS)[number][];
    expect(entries).toHaveLength(3);
    const [first, ...rest] = entries;
    for (const entry of rest) {
      expect(entry.camera).toEqual(first.camera);
    }
  });

  it("(e) Scenario interface accepts optional feaCase string (type-level)", () => {
    // This is a compile-time check: once step-20 adds `feaCase?: string` to
    // the Scenario interface, the cast below should compile without error.
    // At runtime we just verify that the property is either a string or absent
    // on every SCENARIO entry.
    for (const s of SCENARIOS) {
      const feaCase = (s as any).feaCase;
      expect(
        feaCase === undefined || typeof feaCase === "string",
        `scenario '${s.name}': feaCase must be string or undefined, got ${typeof feaCase}`,
      ).toBe(true);
    }
  });

  it("(f) SCENARIOS[0] is still 'm5_geometry_flange' (bootstrap invariant)", () => {
    expect(SCENARIOS[0].name).toBe("m5_geometry_flange");
  });

  it("(g) fea_multi_case_bracket.ri fixture file exists on disk", () => {
    const repoRoot = resolveRepoRoot(import.meta.url);
    const abs = path.join(repoRoot, FEA_FIXTURE);
    expect(
      fs.existsSync(abs),
      `fea_multi_case_bracket.ri not found at ${abs}`,
    ).toBe(true);
  });
});

// ── Task 2968 step s1: RED — cantilever FEA contour scene ────────────────────
//
// These tests FAIL until step s2 adds:
//   - gui/test/fixtures/fea/cantilever_tip_load.ri fixture file
//   - `feaView?: { deformed?: boolean; warp?: number }` to the Scenario interface
//   - the cantilever_contour entry in SCENARIOS

describe("cantilever FEA contour scene (task 2968)", () => {
  it("(a) SCENARIOS contains exactly one entry named 'cantilever_contour'", () => {
    const entries = SCENARIOS.filter((s) => s.name === "cantilever_contour");
    expect(entries).toHaveLength(1);
  });

  it("(b) cantilever_contour fixture is the cantilever_tip_load.ri file", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_contour");
    expect(entry?.fixture).toBe(CANTILEVER_FIXTURE);
  });

  it("(c) cantilever_contour camera has finite 3-number position and target", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_contour");
    expect(entry).toBeDefined();
    const { position, target } = entry!.camera;
    expect(position).toHaveLength(3);
    expect(target).toHaveLength(3);
    for (const v of [...position, ...target]) {
      expect(typeof v).toBe("number");
      expect(isFinite(v)).toBe(true);
    }
  });

  it("(d) cantilever_contour feaView has deformed === false", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_contour");
    expect(entry).toBeDefined();
    expect((entry as any).feaView).toBeDefined();
    expect((entry as any).feaView.deformed).toBe(false);
  });

  it("(e) SCENARIOS[0] is still 'm5_geometry_flange' (bootstrap invariant)", () => {
    expect(SCENARIOS[0].name).toBe("m5_geometry_flange");
  });
});

// ── Task 2968 step s3: RED — cantilever FEA deformed scenes ──────────────────
//
// These tests FAIL until step s4 appends cantilever_deformed_warp1 and
// cantilever_deformed_warp100 to SCENARIOS.

describe("cantilever FEA deformed scenes (task 2968)", () => {
  const WARP_FIXTURE = CANTILEVER_FIXTURE;

  it("(a) SCENARIOS contains 'cantilever_deformed_warp1'", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_deformed_warp1");
    expect(entry).toBeDefined();
  });

  it("(b) SCENARIOS contains 'cantilever_deformed_warp100'", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_deformed_warp100");
    expect(entry).toBeDefined();
  });

  it("(c) cantilever_deformed_warp1 has correct fixture and finite camera", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_deformed_warp1");
    expect(entry).toBeDefined();
    expect(entry!.fixture).toBe(WARP_FIXTURE);
    const { position, target } = entry!.camera;
    expect(position).toHaveLength(3);
    expect(target).toHaveLength(3);
    for (const v of [...position, ...target]) {
      expect(typeof v).toBe("number");
      expect(isFinite(v)).toBe(true);
    }
  });

  it("(d) cantilever_deformed_warp100 has correct fixture and finite camera", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_deformed_warp100");
    expect(entry).toBeDefined();
    expect(entry!.fixture).toBe(WARP_FIXTURE);
    const { position, target } = entry!.camera;
    expect(position).toHaveLength(3);
    expect(target).toHaveLength(3);
    for (const v of [...position, ...target]) {
      expect(typeof v).toBe("number");
      expect(isFinite(v)).toBe(true);
    }
  });

  it("(e) cantilever_deformed_warp1.feaView is {deformed:true, warp:1}", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_deformed_warp1");
    expect(entry).toBeDefined();
    expect((entry as any).feaView).toEqual({ deformed: true, warp: 1 });
  });

  it("(f) cantilever_deformed_warp100.feaView is {deformed:true, warp:100}", () => {
    const entry = SCENARIOS.find((s) => s.name === "cantilever_deformed_warp100");
    expect(entry).toBeDefined();
    expect((entry as any).feaView).toEqual({ deformed: true, warp: 100 });
  });

  it("(g) every feaView.warp value across SCENARIOS is one of [1, 10, 100]", () => {
    const VALID_WARPS = new Set([1, 10, 100]);
    for (const s of SCENARIOS) {
      const feaView = (s as any).feaView;
      if (feaView !== undefined && feaView.warp !== undefined) {
        expect(
          VALID_WARPS.has(feaView.warp),
          `scenario '${s.name}': feaView.warp=${feaView.warp} is not a valid preset (1|10|100)`,
        ).toBe(true);
      }
    }
  });
});
