/**
 * Shared test helpers for async patterns and tree fixtures.
 */
import { vi, expect, type MockInstance } from 'vitest';
import type { EntityTreeNode } from '../types';

// ---------------------------------------------------------------------------
// Tree fixture builders
// ---------------------------------------------------------------------------

/**
 * Create an EntityTreeNode with sensible defaults.
 *
 * `entity_path` is required; all other fields default to the canonical "blank"
 * values used throughout the DesignTree and viewStateStore tests.
 */
export function makeNode(overrides: Partial<EntityTreeNode> & { entity_path: string }): EntityTreeNode {
  return {
    kind: 'structure',
    type_name: null,
    has_mesh: false,
    trait_geometry: false,
    // Default to 'final' so existing tests keep passing without per-call change.
    // Matches the engine-side Freshness::default() = Final (value.rs:2170).
    // Tests that exercise non-Final freshness override this field explicitly.
    freshness: 'final',
    children: [],
    ...overrides,
  };
}

/**
 * Return the canonical Root/A/a1/a2/B tree used by the majority of
 * viewStateStore tests.  Shape: Root { A { a1, a2 }, B }
 * where Root/A/B are `kind='structure'` and a1/a2 are `kind='param'`.
 */
export function makeTree(): EntityTreeNode[] {
  return [
    makeNode({
      entity_path: 'Root',
      kind: 'structure',
      children: [
        makeNode({
          entity_path: 'Root.A',
          kind: 'structure',
          children: [
            makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
            makeNode({ entity_path: 'Root.A.a2', kind: 'param' }),
          ],
        }),
        makeNode({ entity_path: 'Root.B', kind: 'structure' }),
      ],
    }),
  ];
}

/**
 * Return Root { A { a1, a2 }, B { b1, b2 } } — a fully-expanded tree where
 * both A and B have leaf-param children.
 * Used by the `showOnly` and `full PRD integration scenario` describe blocks.
 */
export function makeTreeWithTwoSubtrees(): EntityTreeNode[] {
  return [
    makeNode({
      entity_path: 'Root',
      kind: 'structure',
      children: [
        makeNode({
          entity_path: 'Root.A',
          kind: 'structure',
          children: [
            makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
            makeNode({ entity_path: 'Root.A.a2', kind: 'param' }),
          ],
        }),
        makeNode({
          entity_path: 'Root.B',
          kind: 'structure',
          children: [
            makeNode({ entity_path: 'Root.B.b1', kind: 'param' }),
            makeNode({ entity_path: 'Root.B.b2', kind: 'param' }),
          ],
        }),
      ],
    }),
  ];
}

/**
 * Return Root { A(trait_geometry=true) { a1 }, B } — a tree where A carries
 * `trait_geometry: true` so that geometry-node behaviour can be verified.
 * Used by the `getAllEffective` describe block.
 */
export function makeTreeWithGeometryA(): EntityTreeNode[] {
  return [
    makeNode({
      entity_path: 'Root',
      kind: 'structure',
      children: [
        makeNode({
          entity_path: 'Root.A',
          kind: 'structure',
          trait_geometry: true,
          children: [
            makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
          ],
        }),
        makeNode({ entity_path: 'Root.B', kind: 'structure' }),
      ],
    }),
  ];
}

/**
 * Compute the median of a non-empty array of numbers.
 *
 * Sorts a copy of the input (does not mutate the caller's array) and returns
 * the middle value for odd-length arrays, or the average of the two middle
 * values for even-length arrays.
 *
 * Throws an Error if `values` is empty — a silent NaN would propagate into
 * assertions and produce a confusing failure far from the actual root cause.
 * Also throws if any element is non-finite (NaN, +Infinity, or -Infinity) —
 * `Number.isFinite` subsumes the narrower `Number.isNaN` check and rejects all
 * three cases with a single guard. A non-finite sort result is nondeterministic
 * and would propagate into `toBeLessThan` with a misleading message such as
 * "expected Infinity to be less than 15".
 */
export function median(values: number[]): number {
  if (values.length === 0) {
    throw new Error('median: input array is empty');
  }
  if (values.some(v => !Number.isFinite(v))) {
    throw new Error('median: input contains non-finite value');
  }
  const sorted = [...values].sort((a, b) => a - b);
  const n = sorted.length;
  const mid = Math.floor(n / 2);
  return n % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

/**
 * Format a diagnostic message for wall-clock perf guards.
 *
 * Computes median, min, and max over the sample array and includes a rounded
 * copy of every sample so CI triage can read the distribution at a glance
 * without mentally recomputing statistics from a raw dump.
 *
 * Delegates to `median()` first so non-finite inputs (NaN, ±Infinity) and
 * empty arrays are rejected before any further arithmetic.
 *
 * Min and max are computed via `Array.prototype.reduce` rather than
 * `Math.min(...samples)` / `Math.max(...samples)` to avoid a `RangeError` on
 * very large sample arrays (spread pushes every element onto the call stack).
 *
 * The scalar `median`, `min`, and `max` fields are formatted to exactly two
 * decimal places via `toFixed(2)`.  Sample values in the `samples` array are
 * also rounded to two decimals, but re-coerced to `number` before JSON
 * serialisation, so trailing zeros are dropped (e.g. `1.20` serialises as
 * `1.2`, not `"1.20"`).
 */
export function formatPerfSamples(samples: number[]): string {
  const med = median(samples);
  const min = samples.reduce((a, b) => (b < a ? b : a));
  const max = samples.reduce((a, b) => (b > a ? b : a));
  return `median=${med.toFixed(2)}ms min=${min.toFixed(2)}ms max=${max.toFixed(2)}ms samples=${JSON.stringify(samples.map(v => +v.toFixed(2)))}`;
}

/** Yield to the macrotask queue so setTimeout callbacks execute. */
export const flushMacrotasks = (ms = 0) => new Promise<void>((r) => setTimeout(r, ms));

/** Flush the microtask queue only (no setTimeout). Equivalent to Promise.resolve(). */
export const flushMicrotasks = () => Promise.resolve();

/** Create a Promise whose resolve and reject functions are returned alongside it. */
export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

/**
 * Run `fn` with a temporary `unhandledrejection` handler that calls
 * `preventDefault()`, suppressing test-runner noise from expected
 * unhandled promise rejections.  The handler is removed in a `finally`
 * block so it never leaks across tests.
 */
export async function withSuppressedRejections(fn: () => Promise<void>): Promise<void> {
  const handler = (e: PromiseRejectionEvent) => e.preventDefault();
  window.addEventListener('unhandledrejection', handler);
  try {
    await fn();
  } finally {
    window.removeEventListener('unhandledrejection', handler);
  }
}

/**
 * Run `fn` with a temporary `unhandledrejection` listener (a `vi.fn()` spy)
 * and assert that no unhandled rejections fired during `fn`.  The listener is
 * removed in a `finally` block so it never leaks across tests.
 *
 * Unlike `withSuppressedRejections`, this helper is the *inverse*: it is used
 * when the production code is expected to handle all rejections internally, and
 * any unhandled rejection would represent a regression.  The listener has no
 * `{ once: true }` so every rejection is captured — not just the first.
 */
export async function expectNoUnhandledRejections(fn: () => Promise<void>): Promise<void> {
  const spy = vi.fn();
  window.addEventListener('unhandledrejection', spy);
  try {
    await fn();
    expect(spy).not.toHaveBeenCalled();
  } finally {
    window.removeEventListener('unhandledrejection', spy);
  }
}

async function withSuppressedRejectionsAndConsoleSpy(
  method: 'error' | 'warn',
  fn: (spy: MockInstance) => Promise<void>,
): Promise<void> {
  const spy = vi.spyOn(console, method).mockImplementation(() => {});
  try {
    await withSuppressedRejections(() => fn(spy));
  } finally {
    spy.mockRestore();
  }
}

/**
 * Run `fn` with both a temporary `console.error` spy (output suppressed) and
 * the `unhandledrejection` suppression from `withSuppressedRejections`.
 *
 * The spy is passed as the first argument to `fn` so callers can make
 * targeted assertions (e.g. `expect(errorSpy).not.toHaveBeenCalledWith(...)`).
 * The spy is restored in a `finally` block so it never leaks across tests.
 */
export async function withSuppressedRejectionsAndErrorSpy(
  fn: (errorSpy: MockInstance) => Promise<void>,
): Promise<void> {
  return withSuppressedRejectionsAndConsoleSpy('error', fn);
}

/**
 * Run `fn` with both a temporary `console.warn` spy (output suppressed) and
 * the `unhandledrejection` suppression from `withSuppressedRejections`.
 *
 * The spy is passed as the first argument to `fn` so callers can make
 * targeted assertions (e.g. `expect(warnSpy).toHaveBeenCalledWith(...)`).
 * The spy is restored in a `finally` block so it never leaks across tests.
 */
export async function withSuppressedRejectionsAndWarnSpy(
  fn: (warnSpy: MockInstance) => Promise<void>,
): Promise<void> {
  return withSuppressedRejectionsAndConsoleSpy('warn', fn);
}
