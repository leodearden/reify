/**
 * Shared test helpers for async patterns.
 */

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
