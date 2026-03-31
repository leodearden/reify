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
