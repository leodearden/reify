/**
 * Shared test helpers for the debug-bridge test suite.
 * Centralises the ViewStateStore mock so that adding a newly-reachable
 * bridge method requires updating one place instead of three test files.
 */
import { vi } from 'vitest';
import type { ViewStateStore } from '../stores/viewStateStore';

/**
 * Returns a ViewStateStore mock with vi.fn() stubs for every method
 * currently reachable from bridge handlers, plus `switchView` as a
 * leading indicator for view-management work.
 *
 * When a new bridge handler starts calling another ViewStateStore method,
 * add its stub here — the spy fires rather than throwing
 * "undefined is not a function" at runtime, closing the latent gap that
 * the `as unknown as ViewStateStore` cast would otherwise hide.
 */
export function makeViewStateStoreMock(): ViewStateStore {
  return {
    resetToDefaultView: vi.fn(),
    switchView: vi.fn().mockReturnValue(false),
  } as unknown as ViewStateStore;
}
