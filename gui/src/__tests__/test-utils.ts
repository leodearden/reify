/**
 * Shared test helpers for async patterns.
 */
import { vi, type MockInstance } from 'vitest';

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
