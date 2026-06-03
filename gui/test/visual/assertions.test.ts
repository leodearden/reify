import { describe, it, expect } from "vitest";
import { getByPath } from "./assertions";

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
