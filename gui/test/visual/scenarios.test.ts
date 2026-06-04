import { describe, it, expect } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { SCENARIOS } from "./scenarios.js";
import { resolveRepoRoot } from "./paths.js";

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
