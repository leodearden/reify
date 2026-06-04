import { describe, it, expect } from "vitest";
import { SCENARIOS } from "./scenarios.js";

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
});
