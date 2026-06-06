import { describe, it, expect } from "vitest";
import * as path from "node:path";
import * as fs from "node:fs";
import { getByPath, evaluateAssertion, FIXTURES, VALUE_SCENARIOS, runValueScenario } from "./assertions.js";
import type { Assertion, ValueScenario, ScenarioDeps } from "./assertions.js";
import type { RpcResult } from "./rpc.js";
import { resolveRepoRoot } from "./paths.js";

describe("getByPath", () => {
  it("resolves a two-segment dotted path engine.meshCount", () => {
    expect(getByPath({ engine: { meshCount: 1 } }, "engine.meshCount")).toBe(1);
  });

  it("resolves a single-segment path", () => {
    expect(getByPath({ engine: { meshCount: 1 } }, "engine")).toEqual({ meshCount: 1 });
  });

  it("resolves a multi-segment nested path a.b.c", () => {
    expect(getByPath({ a: { b: { c: 42 } } }, "a.b.c")).toBe(42);
  });

  it("returns undefined for a missing last segment without throwing", () => {
    expect(getByPath({ engine: { meshCount: 1 } }, "engine.missing")).toBeUndefined();
  });

  it("returns undefined for a path through undefined without throwing", () => {
    expect(getByPath({ a: {} }, "a.b.c")).toBeUndefined();
  });

  it("returns undefined for a path through null without throwing", () => {
    expect(getByPath({ a: null }, "a.b.c")).toBeUndefined();
  });
});

describe("evaluateAssertion", () => {
  it("'equals' passes when actual === expected (meshCount 1===1)", () => {
    const a: Assertion = { path: "engine.meshCount", op: "equals", expected: 1 };
    const result = evaluateAssertion({ engine: { meshCount: 1 } }, a);
    expect(result.ok).toBe(true);
  });

  it("'equals' fails with message containing path when actual !== expected (2 !== 1)", () => {
    const a: Assertion = { path: "engine.meshCount", op: "equals", expected: 1 };
    const result = evaluateAssertion({ engine: { meshCount: 2 } }, a);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain("engine.meshCount");
    }
  });

  it("'atLeast' passes when actual >= expected (54 >= 50)", () => {
    const a: Assertion = { path: "count", op: "atLeast", expected: 50 };
    const result = evaluateAssertion({ count: 54 }, a);
    expect(result.ok).toBe(true);
  });

  it("'atLeast' fails with message when actual < expected (3 < 50)", () => {
    const a: Assertion = { path: "count", op: "atLeast", expected: 50 };
    const result = evaluateAssertion({ count: 3 }, a);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain("count");
    }
  });

  it("'exists' passes when path resolves to a defined value (0 is defined)", () => {
    const a: Assertion = { path: "a", op: "exists" };
    const result = evaluateAssertion({ a: 0 }, a);
    expect(result.ok).toBe(true);
  });

  it("'exists' fails with message containing path when path resolves to undefined", () => {
    const a: Assertion = { path: "a", op: "exists" };
    const result = evaluateAssertion({}, a);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain("a");
    }
  });

  // deepEqual contract — nested objects, arrays, key-order insensitivity
  it("'equals' passes for nested objects regardless of key insertion order", () => {
    const a: Assertion = { path: "root", op: "equals", expected: { a: { y: 2, x: 1 } } };
    const result = evaluateAssertion({ root: { a: { x: 1, y: 2 } } }, a);
    expect(result.ok).toBe(true);
  });

  it("'equals' passes for equal arrays in the same order", () => {
    const a: Assertion = { path: "items", op: "equals", expected: [1, 2, 3] };
    const result = evaluateAssertion({ items: [1, 2, 3] }, a);
    expect(result.ok).toBe(true);
  });

  it("'equals' fails when array elements differ", () => {
    const a: Assertion = { path: "items", op: "equals", expected: [1, 2, 3] };
    const result = evaluateAssertion({ items: [1, 2, 4] }, a);
    expect(result.ok).toBe(false);
  });

  it("'equals' fails on array length mismatch", () => {
    const a: Assertion = { path: "items", op: "equals", expected: [1, 2] };
    const result = evaluateAssertion({ items: [1, 2, 3] }, a);
    expect(result.ok).toBe(false);
  });

  it("'equals' fails when array is compared against an object", () => {
    const a: Assertion = { path: "v", op: "equals", expected: { 0: 1 } };
    const result = evaluateAssertion({ v: [1] }, a);
    expect(result.ok).toBe(false);
  });

  // Edge cases: unknown op, non-numeric atLeast
  it("unknown op returns ok===false with 'unknown op' in message", () => {
    // Cast through unknown to simulate a caller passing an unsupported op at runtime
    const a = { path: "x", op: "bogus" } as unknown as Assertion;
    const result = evaluateAssertion({ x: 1 }, a);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain("unknown op");
    }
  });

  it("'atLeast' fails when actual is undefined (missing path)", () => {
    const a: Assertion = { path: "missing", op: "atLeast", expected: 1 };
    const result = evaluateAssertion({ x: 5 }, a);
    expect(result.ok).toBe(false);
  });

  it("'atLeast' fails when actual is a non-numeric string", () => {
    const a: Assertion = { path: "v", op: "atLeast", expected: 1 };
    const result = evaluateAssertion({ v: "hello" }, a);
    expect(result.ok).toBe(false);
  });
});

describe("FIXTURES catalogue", () => {
  it("includes the 5 baseline fixture keys as a subset and all values are .ri paths", () => {
    const keys = Object.keys(FIXTURES);
    for (const known of ["all_severities", "broken_syntax", "empty", "large_assembly", "small_cube"]) {
      expect(keys).toContain(known);
    }
    for (const relPath of Object.values(FIXTURES)) {
      expect(relPath).toMatch(/\.ri$/);
    }
  });

  it("every fixture file exists on disk", () => {
    const repoRoot = resolveRepoRoot(import.meta.url);
    for (const [name, relPath] of Object.entries(FIXTURES)) {
      const absPath = path.join(repoRoot, relPath);
      expect(fs.existsSync(absPath), `fixture '${name}' missing at ${absPath}`).toBe(true);
    }
  });
});

describe("VALUE_SCENARIOS", () => {
  it("is a non-empty array", () => {
    expect(Array.isArray(VALUE_SCENARIOS)).toBe(true);
    expect(VALUE_SCENARIOS.length).toBeGreaterThan(0);
  });

  it("every scenario.fixture is a valid key of FIXTURES", () => {
    const validKeys = Object.keys(FIXTURES);
    for (const scenario of VALUE_SCENARIOS) {
      expect(validKeys).toContain(scenario.fixture);
    }
  });

  it("every scenario has a valid tool name and non-empty assertions array", () => {
    const VALID_OPS = ["equals", "atLeast", "exists"];
    for (const scenario of VALUE_SCENARIOS) {
      expect(typeof scenario.name, `scenario name must be string`).toBe("string");
      expect(scenario.name.length, `scenario name must be non-empty`).toBeGreaterThan(0);
      expect(typeof scenario.tool, `${scenario.name}: tool must be string`).toBe("string");
      expect(scenario.tool.length, `${scenario.name}: tool must be non-empty`).toBeGreaterThan(0);
      expect(Array.isArray(scenario.assertions), `${scenario.name}: assertions must be array`).toBe(true);
      expect(scenario.assertions.length, `${scenario.name}: assertions must be non-empty`).toBeGreaterThan(0);
      for (const assertion of scenario.assertions) {
        expect(typeof assertion.path, `${scenario.name}: assertion.path must be string`).toBe("string");
        expect(VALID_OPS, `${scenario.name}: assertion.op must be valid`).toContain(assertion.op);
      }
    }
  });

  it("contains store_state_meshcount_small_cube with correct shape", () => {
    const scenario: ValueScenario | undefined = VALUE_SCENARIOS.find(
      (s) => s.name === "store_state_meshcount_small_cube",
    );
    expect(scenario).toBeDefined();
    if (scenario) {
      expect(scenario.fixture).toBe("small_cube");
      expect(scenario.tool).toBe("store_state");
      expect(scenario.args).toEqual({});
      expect(scenario.assertions).toContainEqual({
        path: "engine.meshCount",
        op: "equals",
        expected: 1,
      });
    }
  });

  // task-4297 step-7 RED → step-8 GREEN: R2 e2e signal scenarios
  // Existence + uniqueness only — the generic structural tests above already
  // validate fixture/tool/assertions shape. Detailed field pinning would restate
  // the config and break on harmless scenario data edits.
  it("R2 scenarios (get_diagnostics_broken_syntax, ui_outline_small_cube) are both present and unique", () => {
    const r2Names = ["get_diagnostics_broken_syntax", "ui_outline_small_cube"];
    for (const name of r2Names) {
      const matching = VALUE_SCENARIOS.filter((s) => s.name === name);
      expect(matching.length, `${name} should appear exactly once`).toBe(1);
    }
  });

  // task-4304 step-15 RED → step-16 GREEN: F2 LSP probe e2e signal scenarios
  it("F2 probe scenarios are present, unique, and have correct shape", () => {
    const probeNames = [
      "hover_at_markdown_small_cube",
      "completion_at_nonempty_small_cube",
      "definition_at_range_small_cube",
    ];
    const validFixtureKeys = Object.keys(FIXTURES);
    for (const name of probeNames) {
      const matching = VALUE_SCENARIOS.filter((s) => s.name === name);
      expect(matching.length, `${name} should appear exactly once`).toBe(1);
      const scenario = matching[0];
      expect(validFixtureKeys, `${name}: fixture must be a valid FIXTURES key`).toContain(scenario.fixture);
      expect(typeof scenario.args.line, `${name}: args.line must be a number`).toBe("number");
      expect(typeof scenario.args.col, `${name}: args.col must be a number`).toBe("number");
    }
  });

  it("hover_at_markdown_small_cube has atLeast 1 on markdownLength", () => {
    const scenario = VALUE_SCENARIOS.find((s) => s.name === "hover_at_markdown_small_cube");
    expect(scenario).toBeDefined();
    expect(scenario!.tool).toBe("hover_at");
    const assertion = scenario!.assertions.find((a) => a.path === "markdownLength");
    expect(assertion).toBeDefined();
    expect(assertion!.op).toBe("atLeast");
    expect(assertion!.expected).toBeGreaterThanOrEqual(1);
  });

  it("completion_at_nonempty_small_cube has atLeast 1 on itemCount", () => {
    const scenario = VALUE_SCENARIOS.find((s) => s.name === "completion_at_nonempty_small_cube");
    expect(scenario).toBeDefined();
    expect(scenario!.tool).toBe("completion_at");
    const assertion = scenario!.assertions.find((a) => a.path === "itemCount");
    expect(assertion).toBeDefined();
    expect(assertion!.op).toBe("atLeast");
    expect(assertion!.expected).toBeGreaterThanOrEqual(1);
  });

  it("definition_at_range_small_cube has exists on range.start.line", () => {
    const scenario = VALUE_SCENARIOS.find((s) => s.name === "definition_at_range_small_cube");
    expect(scenario).toBeDefined();
    expect(scenario!.tool).toBe("definition_at");
    const assertion = scenario!.assertions.find((a) => a.path === "range.start.line");
    expect(assertion).toBeDefined();
    expect(assertion!.op).toBe("exists");
  });
});

describe("runValueScenario", () => {
  const scenario = VALUE_SCENARIOS.find((s) => s.name === "store_state_meshcount_small_cube")!;

  it("(a) openFixture ok + tool returns engine.meshCount===1 → passed=true, no failures", async () => {
    const deps: ScenarioDeps = {
      openFixture: async (_rel: string): Promise<RpcResult<unknown>> => ({ ok: true, value: null }),
      callTool: async (_tool: string, _args: Record<string, unknown>): Promise<RpcResult<unknown>> => ({
        ok: true,
        value: { engine: { meshCount: 1 } },
      }),
    };
    const result = await runValueScenario(deps, scenario);
    expect(result.passed).toBe(true);
    expect(result.failures).toHaveLength(0);
  });

  it("(b) callTool returns engine.meshCount===2 → passed=false, failure mentions engine.meshCount", async () => {
    const deps: ScenarioDeps = {
      openFixture: async (_rel: string): Promise<RpcResult<unknown>> => ({ ok: true, value: null }),
      callTool: async (_tool: string, _args: Record<string, unknown>): Promise<RpcResult<unknown>> => ({
        ok: true,
        value: { engine: { meshCount: 2 } },
      }),
    };
    const result = await runValueScenario(deps, scenario);
    expect(result.passed).toBe(false);
    expect(result.failures.some((f) => f.includes("engine.meshCount"))).toBe(true);
  });

  it("(c) openFixture fails → passed=false, failure mentions open_file, tool NOT called", async () => {
    let toolCalled = false;
    const deps: ScenarioDeps = {
      openFixture: async (_rel: string): Promise<RpcResult<unknown>> => ({
        ok: false,
        error: "file not found",
      }),
      callTool: async (_tool: string, _args: Record<string, unknown>): Promise<RpcResult<unknown>> => {
        toolCalled = true;
        return { ok: true, value: null };
      },
    };
    const result = await runValueScenario(deps, scenario);
    expect(result.passed).toBe(false);
    expect(result.failures.some((f) => f.includes("open_file"))).toBe(true);
    expect(toolCalled).toBe(false);
  });

  it("(d) callTool returns error → passed=false, failure mentions tool name", async () => {
    const deps: ScenarioDeps = {
      openFixture: async (_rel: string): Promise<RpcResult<unknown>> => ({ ok: true, value: null }),
      callTool: async (_tool: string, _args: Record<string, unknown>): Promise<RpcResult<unknown>> => ({
        ok: false,
        error: "tool failed",
      }),
    };
    const result = await runValueScenario(deps, scenario);
    expect(result.passed).toBe(false);
    expect(result.failures.some((f) => f.includes(scenario.tool))).toBe(true);
  });
});
