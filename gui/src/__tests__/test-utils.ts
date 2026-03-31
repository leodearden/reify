/**
 * Shared test helpers for async patterns.
 */

/** Yield to the macrotask queue so setTimeout(0) callbacks execute. */
export const flushMicrotasks = () => new Promise<void>((r) => setTimeout(r, 0));

/** Create a Promise whose resolve function is returned alongside it. */
export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
}
