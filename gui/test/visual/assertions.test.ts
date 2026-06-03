import { describe, it, expect } from "vitest";
import { getByPath, evaluateAssertion } from "./assertions";
import type { Assertion } from "./assertions";

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
});
