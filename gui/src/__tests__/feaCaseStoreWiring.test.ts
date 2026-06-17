/**
 * Step-9 (RED): feaCaseStoreWiring — inbound fea-case-changed subscription.
 *
 * Tests the `subscribeFeaCaseToStore(store)` wiring helper (task 3026 step-10).
 * The helper calls `onFeaCaseChanged` and routes the payload into
 * `store.applyFeaCaseChanged`.
 *
 * Fails to compile / run until step-10 adds `subscribeFeaCaseToStore` to bridge.ts.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports from mockEvents.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from './test_utils/mockEvents';
import { subscribeFeaCaseToStore } from '../bridge'; // FAILS until step-10
import { createFeaModeStore } from '../stores';
import type { FeaCaseChanged } from '../types';

describe('subscribeFeaCaseToStore (task 3026 η wiring)', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('(a) happy-path: fea-case-changed event populates store.availableCases and activeCaseId', async () => {
    createRoot((dispose) => {
      const store = createFeaModeStore();
      const handle = mockTauriEvent<FeaCaseChanged>('fea-case-changed');

      subscribeFeaCaseToStore(store); // FAILS until step-10

      handle.emit({
        active_case_id: 'operating',
        available_cases: ['operating', 'overload', 'transport'],
      });

      expect(store.state.availableCases).toEqual(['operating', 'overload', 'transport']);
      expect(store.state.activeCaseId).toBe('operating');
      dispose();
    });
  });

  it('(b) happy-path: second event overwrites previous availableCases', async () => {
    createRoot((dispose) => {
      const store = createFeaModeStore();
      const handle = mockTauriEvent<FeaCaseChanged>('fea-case-changed');

      subscribeFeaCaseToStore(store); // FAILS until step-10

      handle.emit({ active_case_id: 'operating', available_cases: ['operating', 'overload'] });
      handle.emit({ active_case_id: 'transport', available_cases: ['operating', 'overload', 'transport'] });

      expect(store.state.availableCases).toEqual(['operating', 'overload', 'transport']);
      expect(store.state.activeCaseId).toBe('transport');
      dispose();
    });
  });

  it('(c) malformed payload (missing available_cases) leaves store untouched', async () => {
    createRoot((dispose) => {
      const store = createFeaModeStore();
      // Pre-set some state to verify immutability
      store.setEnabled(true);
      const handle = mockTauriEvent<unknown>('fea-case-changed');
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

      subscribeFeaCaseToStore(store); // FAILS until step-10

      handle.emit({ active_case_id: 'operating' }); // missing available_cases

      expect(store.state.availableCases).toEqual([]);
      expect(store.state.activeCaseId).toBeNull();
      expect(store.state.enabled).toBe(true); // other fields untouched
      expect(warnSpy).toHaveBeenCalled();
      dispose();
    });
  });

  it('(d) malformed payload (active_case_id not a string) leaves store untouched', async () => {
    createRoot((dispose) => {
      const store = createFeaModeStore();
      const handle = mockTauriEvent<unknown>('fea-case-changed');
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

      subscribeFeaCaseToStore(store); // FAILS until step-10

      handle.emit({ active_case_id: 99, available_cases: ['operating', 'overload'] });

      expect(store.state.availableCases).toEqual([]);
      expect(store.state.activeCaseId).toBeNull();
      expect(warnSpy).toHaveBeenCalled();
      dispose();
    });
  });
});
