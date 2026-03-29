/**
 * Shared test helpers for async patterns.
 */

/** Yield to the macrotask queue so setTimeout(0) callbacks execute. */
export const flushMicrotasks = () => new Promise<void>((r) => setTimeout(r, 0));
